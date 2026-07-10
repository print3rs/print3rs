use {
    crate::commands::log::{Segment, get_headers, make_parser},
    print3rs_core::{Error as PrinterError, Printer, Socket},
    std::{
        collections::HashMap,
        time::{SystemTime, UNIX_EPOCH},
    },
    tokio::{io::AsyncWriteExt, task::JoinHandle},
    winnow::Parser,
};

/// Starts a background task which reads a .gcode file and sends the commands in sequence
pub fn start_print_file(filename: &str, socket: Socket) -> BackgroundTask {
    let filename = filename.to_owned();
    let task: JoinHandle<Result<(), TaskError>> = tokio::spawn(async move {
        if let Ok(file) = tokio::fs::read_to_string(filename).await {
            for line in file.lines() {
                let line = match line.split_once(';') {
                    Some((s, _)) => s,
                    None => line,
                };
                if line.is_empty() {
                    continue;
                };
                socket.send(line).await?.await?;
            }
        }
        Ok(())
    });
    BackgroundTask {
        description: "print",
        abort_handle: task.abort_handle(),
    }
}

#[derive(Debug, thiserror::Error)]
enum TaskError {
    #[error("{0}")]
    Printer(#[from] print3rs_core::Error),
    #[error("failed in background: {0}")]
    Join(#[from] tokio::task::JoinError),
}

/// Starts a background task which listens for a pattern an writes it in a file
pub fn start_logging(
    name: &str,
    pattern: Vec<Segment<&'_ str>>,
    printer: &Printer,
) -> std::result::Result<BackgroundTask, print3rs_core::Error> {
    let filename = format!(
        "{name}_{timestamp}.csv",
        timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    );
    let header = get_headers(&pattern);

    let mut parser = make_parser(pattern);
    let mut log_printer_reader = printer.subscribe_lines()?;
    let log_task_handle = tokio::spawn(async move {
        let mut log_file = tokio::fs::File::create(filename).await.unwrap();
        log_file.write_all(header.as_bytes()).await.unwrap();
        while let Ok(log_line) = log_printer_reader.recv().await {
            if let Ok(parsed) = parser.parse(log_line.as_bytes()) {
                let mut record_bytes = String::new();
                for val in parsed {
                    record_bytes.push_str(&val.to_string());
                    record_bytes.push(',');
                }
                record_bytes.pop(); // remove trailing ','
                record_bytes.push('\n');
                log_file
                    .write_all(record_bytes.as_bytes())
                    .await
                    .unwrap_or_default();
            }
        }
    });
    Ok(BackgroundTask {
        description: "log",
        abort_handle: log_task_handle.abort_handle(),
    })
}

/// Starts a background task sending Gcodes one-at-a-time in an infinite loop
pub fn start_repeat(gcodes: Vec<String>, socket: Socket) -> BackgroundTask {
    let task: JoinHandle<Result<(), TaskError>> = tokio::spawn(async move {
        for ref line in gcodes.into_iter().cycle() {
            let _ = socket.send_unsequenced(line).await?.await;
        }
        Ok(())
    });
    BackgroundTask {
        description: "repeat",
        abort_handle: task.abort_handle(),
    }
}

pub type Tasks = HashMap<String, BackgroundTask>;

/// Handle for a concurrent task with description.
/// Task is cancelled on drop.
#[derive(Debug)]
pub struct BackgroundTask {
    pub description: &'static str,
    pub abort_handle: tokio::task::AbortHandle,
}

impl Drop for BackgroundTask {
    fn drop(&mut self) {
        self.abort_handle.abort()
    }
}

/// Starts a background task which sends given Gcodes one-at-a-time
pub fn send_gcodes(socket: Socket, codes: Vec<String>) -> BackgroundTask {
    let task: JoinHandle<Result<(), PrinterError>> = tokio::spawn(async move {
        for code in codes {
            let _ = socket.send_unsequenced(code).await?.await;
        }
        Ok(())
    });
    BackgroundTask {
        description: "gcodes",
        abort_handle: task.abort_handle(),
    }
}
