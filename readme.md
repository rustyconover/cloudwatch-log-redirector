# CloudWatch Log Redirector

Welcome to the **CloudWatch Log Redirector**! This handy command-line tool helps you capture the logs from your applications and redirect them to AWS CloudWatch Logs.

## Why Use CloudWatch Logs?

CloudWatch Logs is a fantastic service for storing application logs, especially in environments like Docker or ECS containers. In these scenarios, local storage is often unavailable, making CloudWatch an ideal solution for log management. However, there are times when you want to capture logs from specific applications running alongside others within the same container or environment. That's where the CloudWatch Log Redirector comes in!

## Features

- **Redirect Logs**: Capture both STDOUT and STDERR from your applications and send them directly to CloudWatch Logs.
- **Stream Tagging**: Easily identify logs by tagging them with `[STDOUT]` and `[STDERR]`.
- **Real-Time Monitoring**: Use the `--tee` option to view logs in real time while they are sent to CloudWatch.

## How to Use

Getting started is simple! Here’s how to use the CloudWatch Log Redirector:

```
Usage: cloudwatch-log-redirector [OPTIONS] <LOG_GROUP_NAME> <LOG_STREAM_NAME> <COMMAND> [ARGS]...

Arguments:
  <LOG_GROUP_NAME>   The name of the CloudWatch Logs log group.
  <LOG_STREAM_NAME>  The name of the CloudWatch Logs log stream.
  <COMMAND>          The command to execute.
  [ARGS]...          Additional arguments for the command.

Options:
  -t, --tag-stream-names  Tag the stream names with [STDOUT] and [STDERR].
      --tee               Display output in real time while sending it to CloudWatch Logs.
  -h, --help              Show this help message.
  -V, --version           Display the current version of the tool.
```

### Example Usage

Here’s a quick example of how to use the tool:

```bash
cloudwatch-log-redirector my-log-group my-log-stream my-command --arg1 value1 --arg2 value2
```

## How It Works

The CloudWatch Log Redirector is built with **Tokio**, allowing it to efficiently manage subprocesses. When you run a command, the tool does the following:

1. **Starts the Subprocess**: It spawns your command as a subprocess.
2. **Attaches Pipes**: It connects pipes to capture both STDOUT and STDERR.
3. **Reads Logs**: Two tasks read from the respective pipes and place the logs into a queue.
4. **Flushes to CloudWatch**: The queued messages are periodically sent to CloudWatch Logs.
5. **Exit Code**: When the command completes, the exit code of the redirector matches that of the child process.

## Getting Help

Need more assistance? Just run:

```bash
cloudwatch-log-redirector --help
```

to view the available options and usage instructions.

