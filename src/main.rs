#[macro_use]
extern crate clap;
#[macro_use]
extern crate error_chain;
#[macro_use]
extern crate serde;

error_chain!(
    foreign_links {
        ReqwestError(reqwest::Error);
    }
);

quick_main!(run);

mod run;
mod docker;

/// A macro that will execute the provided code when it goes out of scope.
macro_rules! finally {
    ($contents:block) => {
        struct A<F: Fn() -> ()> {c : F};

        impl<F> Drop for A<F> where F: Fn() -> () {
            fn drop(&mut self) {
                (self.c)();
            }
        }

        #[allow(unused)]
        let a = A{c: || { $contents },};
    };
}

fn run() -> Result<()> {
    env_logger::init();

    let cli_arg_matches = clap::App::new(crate_name!())
        .version(crate_version!())
        .about(crate_description!())
        .author(crate_authors!())
        .setting(clap::AppSettings::ArgRequiredElseHelp)
        .arg_from_usage("--image=[image] 'Docker image to use'")
        .arg_from_usage("--container=[container] 'Docker container to use'")
        .arg_from_usage("-p --port 'Port of the CDS Server'")
        .arg_from_usage("-i --input=<input> 'File containing the problem (required)'")
        .arg_from_usage("-o --output=[output] 'File containing the expected output/solution'")
        .arg_from_usage("-m --measure 'Measure-mode, run multiple times and output csv-like data (duration in micro seconds)'")
        .arg_from_usage("-c --cpus=[cpus] 'Number of cpus the application is allowed to use (can be a comma-separated list in measure-mode) [default: 1,2,4]'")
        .arg_from_usage("-r --runs=[runs] 'Number of runs to average the program runtime (only in measure-mode) [default: 3]'")
        .arg_from_usage("<program> 'The program to start'")
        .group(clap::ArgGroup::with_name("unit-under-test")
            .args(&["image", "container"])
            .required(true))
        .get_matches();

    docker::check()
        .chain_err(|| "Can not run docker commands!")?;

    let program = cli_arg_matches
        .value_of("program")
        .expect("program is strictly necessary");

    let port: u16 = cli_arg_matches
        .value_of("port")
        .unwrap_or("8080")
        .parse()
        .chain_err(|| "Unable to parse given port")?;

    let input_path = cli_arg_matches
        .value_of("input")
        .expect("input is strictly necessary");

    let input = {
        use std::io::Read;
        let mut input_file = std::fs::File::open(input_path)
            .chain_err(|| format!("Unable to open input file {} containing the problem to be solved", input_path))?;
        let mut input: Vec<u8> = Vec::new();
        input_file
            .read_to_end(&mut input)
            .chain_err(|| format!("Unable to read content of input file {}", input_path))?;
        input
    };

    let expected_output = {
        if let Some(output_path) = cli_arg_matches.value_of("output") {
            let mut output_file = std::fs::File::open(output_path)
                .chain_err(|| format!("Unable to open the file containing the expected output (solution) {}", output_path))?;
            let mut output = String::new();
            std::io::Read::read_to_string(&mut output_file, &mut output)
                .chain_err(|| format!("Unable to read content of output file {}", output_path))?;
            Some(output)
        } else {
            None
        }
    };

    if cli_arg_matches.is_present("measure") {
        let image_id = cli_arg_matches
            .value_of("image")
            .expect("image is strictly necessary in measure-mode!");
        let runs = cli_arg_matches
            .value_of("runs")
            .unwrap_or("3")
            .parse()
            .chain_err(|| "Given runs is not a number")?;
        let cpu_counts = cli_arg_matches
            .value_of("cpus")
            .unwrap_or("1,2,4")
            .split(",")
            .fold(Ok(Vec::new()), |v: Result<Vec<u32>>, cpu_count: &str| {
                match v {
                    Ok(mut v) => {
                        v.push(
                            cpu_count.parse()
                                .chain_err(|| format!("Provided CPU count {} is not a positive number", cpu_count))?
                        );
                        Ok(v)
                    },
                    Err(e) => Err(e)
                }
            })?;

        let mut have_header_already = false;
        for r in 0..runs {
            for cpu_count in cpu_counts.iter() {
                // generate cpuset argument
                let cpuset = (0..*cpu_count)
                    .map(|x| format!("{}", x))
                    .collect::<Vec<String>>()
                    .join(",");
                
                // start benchmark container with restricted cpus
                let cid = docker::start_container(
                        image_id,
                        &["--cpuset-cpus", cpuset.as_str(),
                        "-p", port.to_string().as_str(),
                        "-e", format!("MAX_CPUS={}", cpu_count).as_str(),
                        "-e", format!("CDS_PORT={}", port).as_str(),
                        "-e", "RUST_LOG=debug",
                        "-e", format!("REST_PORT={}", port).as_str()
                ])
                    .chain_err(|| "Starting of measurement container failed")?;

                // ensure container does not keep running
                // TODO The finally! macro apparently doesn't work anymore with the new rust version
                // directly terminating the container after it started.
                // finally! {{
                //     docker::stop_container(cid.as_str(), false);
                // }}

                let (stdout, stderr, exit_status, duration) = run::run(cid.as_str(), port, program, input.as_slice())
                    .chain_err(|| "Measuring failed")?;

                if exit_status != 0 {
                    bail!("Measurement run terminated with non-zero exit status! exit_status: {}\nstdout:\n--------------\n{}--------------\nstderr:\n--------------\n{}--------------",
                        exit_status,
                        stdout,
                        stderr
                    );
                }

                if let Some(ref eout) = expected_output {
                    if eout != &stdout {
                        bail!("Measurement run terminated with unexpected output. actual stdout:\n--------------\n{}--------------\nexpected stdout:\n--------------\n{}--------------",
                            stdout,
                            eout
                        );
                    }
                }

                docker::stop_container(cid.as_str(), true)
                    .chain_err(|| "Deleting container failed!")?;

                if ! have_header_already {
                    println!("program; run; cpus; duration;");
                    have_header_already = true;
                }
                println!("{}; {}; {}; {}", program, r, cpu_count, duration);
            }
        }
    } else {
        let (cid, created) = if let Some(image_id) = cli_arg_matches.value_of("image") {
            // if cli_arg_matches.find(",").is_some() {
            //     bail!("multiple, comma separated --cpus argument are only allowed in measurement-mode");
            // }

            let cpu_count = cli_arg_matches
                .value_of("cpus")
                .unwrap_or("4")
                .parse::<u32>()
                .chain_err(|| "Given cpu count is not a positive number")?;

            let cpuset = (0..cpu_count)
                .map(|x| format!("{}", x))
                .collect::<Vec<String>>()
                .join(",");

            let cid = docker::start_container(
                image_id,
                &["--cpuset-cpus", cpuset.as_str(),
                "-p", port.to_string().as_str(),
                "-e", format!("MAX_CPUS={}", cpu_count).as_str(),
                "-e", format!("REST_PORT={}", port).as_str()
            ])
                .chain_err(|| "starting of measurement container failed")?;
            (cid, true)
        } else {
            (cli_arg_matches.value_of("container").unwrap().to_string(), false)
        };

        finally! {{
            if created {
                print!("Stopping container ... ");
                docker::stop_container(cid.as_str(), true);
                println!("DONE");
            }
        }}

        let (stdout, stderr, exit_status, duration) = run::run(cid.as_str(), port, program, input.as_slice())
            .chain_err(|| "Measuring failed")?;

        println!("Ran program {}\nExit status: {}\nDuration: {} micro seconds\nstdout:\n--------------\n{}--------------\nstderr:\n--------------\n{}--------------",
            program,
            exit_status,
            duration,
            stdout,
            stderr
        );

        if let Some(ref eout) = expected_output {
            if eout != &stdout {
                bail!("Actual output differs from expected output:\n--------------\n{}--------------",
                    eout
                );
            }
        }
    }

    Ok(())
}