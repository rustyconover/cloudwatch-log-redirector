use aws_config::meta::region::RegionProviderChain;
use aws_config::BehaviorVersion;
use aws_sdk_cloudwatchlogs as cloudwatchlogs;
use aws_sdk_cloudwatchlogs::types::InputLogEvent;
use clap::Parser;
use cloudwatchlogs::{Client, Error};
use std::process::exit;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::sync::Mutex;
use tokio::time::{self, Duration};
struct LogBuffer {
    buffer: Vec<(InputLogEvent, u64)>,
    counter: u64,
}

impl LogBuffer {
    // Create a new LogBuffer instance
    fn new() -> Self {
        Self {
            buffer: Vec::new(),
            counter: 0,
        }
    }

    // Add a log event to the buffer
    fn add_event(&mut self, event: InputLogEvent) {
        self.buffer.push((event, self.counter));
        self.counter += 1;
    }

    /// A function that sorts log events by timestamp and returns a subset whose total size is < 1MB.
    /// The returned subset is removed from the original vector.
    fn get_subset_to_publish(&mut self) -> Vec<InputLogEvent> {
        // Sort the log events by timestamp in ascending order
        self.buffer
            .sort_by_key(|event| (event.0.timestamp(), event.1));

        // Define the maximum size in bytes (1MB = 1 * 1024 * 1024 bytes)
        const MAX_SIZE: usize = 1024 * 1024;

        let mut subset = Vec::new();
        let mut total_size = 0;

        // Use the `retain` method to remove events that are part of the subset from the original vector
        self.buffer.retain(|event| {
            let message_size = event.0.message().len();

            // If adding this event would exceed the size limit, return true (keep the event)
            if total_size + message_size >= MAX_SIZE || subset.len() >= 1000 {
                return true;
            }

            // Otherwise, add the event to the subset and accumulate the size
            total_size += message_size;
            subset.push(event.clone());

            // Return false to remove this event from the original vector
            false
        });

        subset.iter().map(|(event, _)| event.clone()).collect()
    }

    // Flush the buffered log events
}

#[derive(Parser)]
#[command(name = "cloudwatch_output_redirector")]
#[command(version = "1.0")]
#[command(author = "Rusty Conover <rusty@conover.me>")]
#[command(about = "Redirects stdout and stderr to CloudWatch Logs")]
struct CommandLineArgs {
    /// The name of the CloudWatch Logs log group
    log_group_name: String,

    /// The name of the CloudWatch Logs log stream
    log_stream_name: String,

    #[arg(
        short,
        long,
        default_value_t = true,
        help = "Tag the stream names with [STDOUT] and [STDERR]"
    )]
    tag_stream_names: bool,

    #[arg(
        name = "tee",
        long,
        default_value_t = false,
        help = "Tee the output rather than just sending it to CloudWatch Logs"
    )]
    tee: bool,

    /// The command to execute
    command: String,

    /// Arguments for the command
    args: Vec<String>,
}

fn current_time_in_millis() -> i64 {
    // Get the current time since the UNIX_EPOCH
    let now = SystemTime::now();

    // Calculate the duration since UNIX_EPOCH
    now.duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as i64
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt::init();

    // Parse command line arguments
    let args = CommandLineArgs::parse();

    // Create a CloudWatch Logs client
    let region_provider = RegionProviderChain::default_provider().or_else("us-east-1"); // Adjust your region
    let config = Arc::new(
        aws_config::defaults(BehaviorVersion::v2024_03_28())
            .region(region_provider)
            .load()
            .await,
    );

    // Create log group and stream
    {
        let client = Client::new(&Arc::clone(&config));

        if let Err(e) = create_log_group(&client, &args.log_group_name).await {
            if e.to_string().contains("ResourceAlreadyExistsException") {
                tracing::debug!("Log group already exists");
            } else {
                tracing::error!("Error creating log group: {}", e);
            }
        }
        if let Err(e) =
            create_log_stream(&client, &args.log_group_name, &args.log_stream_name).await
        {
            if e.to_string().contains("ResourceAlreadyExistsException") {
                tracing::debug!("Log stream already exists");
            } else {
                tracing::error!("Error creating log group: {}", e);
            }
        }
    }

    // Start the subprocess
    let mut child = match tokio::process::Command::new(&args.command)
        .args(&args.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => {
            println!("Successfully started child process: {}", args.command);
            // You can now interact with `child`, e.g., read stdout/stderr
            child
        }
        Err(e) => {
            eprintln!(
                "Error: Failed to start the child process '{}': {}",
                args.command, e
            );
            exit(1);
        }
    };

    // Create a buffer for the output streams
    let stdout = child.stdout.take().expect("Failed to capture stdout");
    let stderr = child.stderr.take().expect("Failed to capture stderr");

    let stdout_reader = BufReader::new(stdout);
    let stderr_reader = BufReader::new(stderr);

    let log_buffer = Arc::new(Mutex::new(LogBuffer::new()));

    let exit_indicator = Arc::new(Mutex::new(false));

    // Stream stdout
    let stdout_buffer = Arc::clone(&log_buffer);
    let stdout_task = tokio::spawn(async move {
        let mut lines = stdout_reader.lines();
        while let Some(line) = lines.next_line().await.unwrap() {
            let m = if args.tag_stream_names {
                format!("[STDOUT] {}", line)
            } else {
                line.clone()
            };
            let log_event = InputLogEvent::builder()
                .message(m)
                .timestamp(current_time_in_millis())
                .build()
                .unwrap();

            if args.tee {
                println!("{}", line);
            }

            {
                let mut buffer = stdout_buffer.lock().await;
                buffer.add_event(log_event);
            }
        }
    });

    let stderr_buffer = Arc::clone(&log_buffer);
    let stderr_task = tokio::spawn(async move {
        let mut lines = stderr_reader.lines();
        while let Some(line) = lines.next_line().await.unwrap() {
            let m = if args.tag_stream_names {
                format!("[STDERR] {}", line)
            } else {
                line.clone()
            };

            let log_event = InputLogEvent::builder()
                .message(m)
                .timestamp(current_time_in_millis())
                .build()
                .unwrap();

            if args.tee {
                eprintln!("{}", line);
            }

            {
                let mut buffer = stderr_buffer.lock().await;
                buffer.add_event(log_event);
            }
        }
    });

    let flush_buffer = Arc::clone(&log_buffer);
    let exit_flag_handle = Arc::clone(&exit_indicator);
    let flush_handle = tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_millis(250));
        let client = Client::new(&config);
        let mut skip_sleep = false;
        loop {
            if !skip_sleep {
                interval.tick().await; // Wait for the next tick
            }

            let messages = {
                // Acquire the lock here but release it quickly
                let mut buffer = flush_buffer.lock().await;
                buffer.get_subset_to_publish()
            };

            if messages.is_empty() {
                tracing::trace!("Nothing to flush");
                let exit = exit_flag_handle.lock().await;
                if *exit {
                    tracing::debug!("Exiting flush loop due to flag being set");
                    break;
                }
                skip_sleep = false;
                continue; // Nothing to flush
            }

            if messages.len() > 100 {
                skip_sleep = true;
            }

            tracing::debug!("Flushing {} log events", messages.len());
            let result = client
                .put_log_events()
                .log_group_name(&args.log_group_name)
                .log_stream_name(&args.log_stream_name)
                .set_log_events(Some(messages.clone()))
                .send()
                .await;
            match result {
                Ok(put_result) => {
                    if put_result.rejected_log_events_info.is_some() {
                        tracing::warn!("Some log events were rejected");
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to flush log events: {}", e);
                }
            }
        }
    });

    // Wait for the child process to finish
    let child_status = child.wait().await;
    match &child_status {
        Ok(exit_status) => {
            tracing::debug!("Process finished with exit status: {}", exit_status);
        }
        Err(e) => {
            tracing::error!("Failed to wait for child process: {}", e);
        }
    }

    match stdout_task.await {
        Ok(_) => {}
        Err(e) => {
            tracing::error!("Failed to read stdout: {}", e);
        }
    }
    match stderr_task.await {
        Ok(_) => {}
        Err(e) => {
            tracing::error!("Failed to read stderr: {}", e);
        }
    }

    // Set the exit indicator to true
    tracing::debug!("Setting exit indicator so final messages are flushed.");
    {
        let mut exit = exit_indicator.lock().await;
        *exit = true;
    }

    // Flush any pending events.
    let flush_result = flush_handle.await;
    if let Err(e) = flush_result {
        tracing::error!("Failed to flush log events: {}", e);
    }

    if let Ok(exit_status) = &child_status {
        // Exit with the same status code as the child process
        exit(exit_status.code().unwrap());
    }

    Ok(())
}

// Function to create a CloudWatch log group
async fn create_log_group(client: &Client, log_group_name: &str) -> Result<(), Error> {
    client
        .create_log_group()
        .log_group_name(log_group_name)
        .send()
        .await?;
    Ok(())
}

// Function to create a CloudWatch log stream
async fn create_log_stream(
    client: &Client,
    log_group_name: &str,
    log_stream_name: &str,
) -> Result<(), Error> {
    client
        .create_log_stream()
        .log_group_name(log_group_name)
        .log_stream_name(log_stream_name)
        .send()
        .await?;
    Ok(())
}
