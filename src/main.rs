use serde::Deserialize;
use serde_json;
use tokio::fs::remove_file;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tokio::task;

use time;

use crossterm::event;

#[derive(Deserialize, PartialEq, Eq, Hash, Clone)]
struct JobId(String);

enum MainLoopMessage {
    AllOk,
    SocketMessage(JobId, JobMessage),
    NewConnection(FirstJobMessage, mpsc::Sender<MainToConnectionMessage>),
    ConnectionDropped(JobId),
    KeyPressed(event::KeyCode),
}

enum MainToConnectionMessage {
    AllOk,
    CancelJob,
}

enum MainToListenerMessage {
    Shutdown,
}

enum MainToInterfaceMessage {
    AllOk,
}
enum InterfaceToMainMessage {
    Text(String),
}

#[derive(Deserialize)]
struct FirstJobMessage {
    total_epochs: usize,
    id: JobId,
}

#[derive(Deserialize)]
struct JobMessage {
    #[serde(default)]
    current_epoch: Option<usize>,
    #[serde(default)]
    total_epochs: Option<usize>,
    #[serde(default)]
    message: Option<String>,
}

#[tokio::main]
async fn main() {
    let (
        interface_handle,
        listener_handle,
        mut connection_to_main_receiver,
        main_to_listener_sender,
    ) = initialize_process();

    let mut all_senders: std::collections::HashMap<JobId, mpsc::Sender<MainToConnectionMessage>> =
        std::collections::HashMap::new();

    let mut app = App::new();

    let mut terminal = ratatui::init();
    loop {
        terminal
            .draw(|frame| {
                app.draw(frame);
            })
            .unwrap();

        let mut break_loop = false;
        let message = connection_to_main_receiver.recv().await;

        match message.unwrap() {
            MainLoopMessage::SocketMessage(sender, msg) => {
                app.add_connection_message(sender, msg);
            }
            MainLoopMessage::NewConnection(first_message, sender) => {
                all_senders.insert(first_message.id.clone(), sender);
                app.current_jobs.push(CurrentJob {
                    id: first_message.id,
                    messages: Vec::new(),
                    current_epoch: 0,
                    total_epochs: first_message.total_epochs,
                })
            }
            MainLoopMessage::ConnectionDropped(sender) => {
                app.finish_connection(sender);
            }
            MainLoopMessage::KeyPressed(key_code) => match key_code {
                event::KeyCode::Backspace => {
                    app.current_input.pop();
                }
                event::KeyCode::Enter => break_loop = true,
                event::KeyCode::Char(x) => {
                    app.current_input.push(x);
                }
                _ => {}
            },
            _ => {}
        };

        if break_loop {
            break;
        }
    }

    terminal
        .draw(|frame| {
            frame.render_widget(
                ratatui::widgets::Paragraph::new("Press any key to exit"),
                frame.area(),
            )
        })
        .unwrap();

    drop(connection_to_main_receiver);

    main_to_listener_sender
        .send(MainToListenerMessage::Shutdown)
        .await
        .unwrap();
    listener_handle.await.unwrap();
    interface_handle.join().unwrap();
    ratatui::restore();
}

fn initialize_process() -> (
    std::thread::JoinHandle<()>,
    task::JoinHandle<()>,
    mpsc::Receiver<MainLoopMessage>,
    mpsc::Sender<MainToListenerMessage>,
) {
    let (connection_to_main_sender, connection_to_main_receiver) =
        mpsc::channel::<MainLoopMessage>(300);

    let (main_to_listener_sender, main_to_listener_receiver) =
        mpsc::channel::<MainToListenerMessage>(300);

    let interface_to_main_sender = connection_to_main_sender.clone();

    let listener_handle = task::spawn(async move {
        listen_port(main_to_listener_receiver, connection_to_main_sender).await
    });

    let interface_handle = std::thread::spawn(move || interface_loop(interface_to_main_sender));

    return (
        interface_handle,
        listener_handle,
        connection_to_main_receiver,
        main_to_listener_sender,
    );
}

async fn listen_port(
    mut main_to_listener_receiver: mpsc::Receiver<MainToListenerMessage>,
    connection_to_main_sender: mpsc::Sender<MainLoopMessage>,
) {
    if let Err(_) = tokio::fs::remove_file("the_socket").await {};
    let listener = UnixListener::bind("the_socket").unwrap();
    loop {
        let break_loop: bool = tokio::select! {
            stream = listener.accept() => {
                let stream = stream.unwrap().0;
                let cloned_main_sender = connection_to_main_sender.clone();
                task::spawn(async move { handle_connection(stream, cloned_main_sender).await });
                false
            },
            _message = main_to_listener_receiver.recv() => {
                // TODO: Handle more messages?
                true
            }
        };
        if break_loop {
            break;
        }
    }
    remove_file("the_socket").await.ok();
}

async fn handle_connection(
    mut stream: UnixStream,
    connection_to_main_sender: mpsc::Sender<MainLoopMessage>,
) {
    let (main_to_connection_sender, mut main_to_connection_receiver) = mpsc::channel(500);

    let mut buffer: [u8; 500] = [0; 500];
    let n_bytes = stream.read(&mut buffer).await.unwrap();
    let first_message: FirstJobMessage =
        serde_json::from_str(std::str::from_utf8(&buffer[..n_bytes]).unwrap()).unwrap();

    let socket_name = first_message.id.clone();

    connection_to_main_sender
        .send(MainLoopMessage::NewConnection(
            first_message,
            main_to_connection_sender,
        ))
        .await
        .unwrap();

    let mut buffer: [u8; 1024] = [0; 1024];
    loop {
        let break_loop = tokio::select! {
            n_bytes = stream.read(&mut buffer) => {
                let n_bytes = n_bytes.unwrap();
                let message = std::str::from_utf8(&buffer[..n_bytes]).unwrap().to_owned();
                if message == "FINISH" {
                    connection_to_main_sender
                        .send(MainLoopMessage::ConnectionDropped(socket_name.clone()))
                        .await
                        .unwrap();
                    true
                } else {
                    connection_to_main_sender
                        .send(MainLoopMessage::SocketMessage(socket_name.clone(), serde_json::from_str(&message).unwrap()))
                        .await
                        .unwrap();
                    false
                }
            },
            message = main_to_connection_receiver.recv() => {
                match message.unwrap() {
                    MainToConnectionMessage::AllOk => false,
                    MainToConnectionMessage::CancelJob => {
                        true
                    }
                }
            }
        };
        if break_loop {
            break;
        }
    }
}

fn interface_loop(interface_to_main_sender: mpsc::Sender<MainLoopMessage>) {
    loop {
        if let event::Event::Key(key) = event::read().unwrap() {
            if key.kind != event::KeyEventKind::Press {
                continue;
            }
            if let Err(_) =
                interface_to_main_sender.blocking_send(MainLoopMessage::KeyPressed(key.code))
            {
                break;
            }
        }
    }
}

struct FinishedJob {
    id: JobId,
    end_time: String,
}

struct CurrentJob {
    id: JobId,
    messages: Vec<String>,
    current_epoch: usize,
    total_epochs: usize,
}

struct App {
    current_input: String,
    finished_jobs: Vec<FinishedJob>,
    current_jobs: Vec<CurrentJob>,
}

impl App {
    fn new() -> Self {
        App {
            current_input: String::new(),
            finished_jobs: Vec::new(),
            current_jobs: Vec::new(),
        }
    }

    fn add_new_connection(&mut self, connection_name: String) {
        self.current_jobs.push(CurrentJob {
            id: JobId(connection_name),
            messages: Vec::new(),
            current_epoch: 0,
            total_epochs: 1,
        })
    }

    fn add_connection_message(&mut self, connection_name: JobId, message: JobMessage) {
        for job in self.current_jobs.iter_mut() {
            if job.id.0 == connection_name.0 {
                if let Some(x) = message.message {
                    job.messages.push(x);
                }
                if let Some(x) = message.current_epoch {
                    job.current_epoch = x;
                }
                if let Some(x) = message.total_epochs {
                    job.total_epochs = x;
                }
                break;
            }
        }
    }

    fn finish_connection(&mut self, connection_name: JobId) {
        let mut index_to_remove = None;
        for (i, job_name) in self.current_jobs.iter().map(|x| &x.id).enumerate() {
            if job_name == &connection_name {
                index_to_remove = Some(i);
                break;
            }
        }
        self.current_jobs.remove(index_to_remove.unwrap());

        let end_time = time::OffsetDateTime::now_utc();

        self.finished_jobs.push(FinishedJob {
            id: connection_name.clone(),
            end_time: format!("{:0>2}:{:0>2}", end_time.hour(), end_time.minute()),
        });
    }

    fn draw(&self, frame: &mut ratatui::Frame) {
        let [main_area, bottom_area] = ratatui::layout::Layout::vertical([
            ratatui::layout::Constraint::Min(1),
            ratatui::layout::Constraint::Max(2),
        ])
        .areas(frame.area());

        let finished_jobs_height = 7 + self.finished_jobs.len().min(5) as u16;

        let [finished_jobs_title, finished_jobs_area, _, current_jobs_title, current_jobs_area] =
            ratatui::layout::Layout::vertical([
                ratatui::layout::Constraint::Max(1),
                ratatui::layout::Constraint::Max(finished_jobs_height),
                ratatui::layout::Constraint::Max(1),
                ratatui::layout::Constraint::Max(1),
                ratatui::layout::Constraint::Min(3),
            ])
            .areas(main_area);

        frame.render_widget(
            ratatui::widgets::Paragraph::new("FINISHED JOBS")
                .alignment(ratatui::layout::Alignment::Center),
            finished_jobs_title,
        );

        let rows: Vec<ratatui::widgets::Row> = self
            .finished_jobs
            .iter()
            .enumerate()
            .map(|(i, x)| {
                ratatui::widgets::Row::new(vec![
                    format!("{}", i + 1),
                    x.id.0.clone(),
                    x.end_time.clone(),
                ])
            })
            .collect();

        let finished_padding = ratatui::widgets::block::Padding::new(1, 0, 1, 1);
        let finished_jobs_widget = ratatui::widgets::Table::new(rows, [2, 15, 15])
            .block(ratatui::widgets::Block::new().padding(finished_padding));

        frame.render_widget(finished_jobs_widget, finished_jobs_area);

        frame.render_widget(
            ratatui::widgets::Paragraph::new("RUNNING JOBS")
                .alignment(ratatui::layout::Alignment::Center),
            current_jobs_title,
        );

        let current_jobs_split_areas = ratatui::layout::Layout::vertical(
            std::iter::repeat(ratatui::layout::Constraint::Max(10)).take(self.current_jobs.len()),
        )
        .split(current_jobs_area);

        for (i, job) in self.current_jobs.iter().enumerate() {
            // let finished_padding = ratatui::widgets::block::Padding::new(3, 0, 1, 1);

            let finished_block =
                ratatui::widgets::Block::bordered().title(self.current_jobs[i].id.0.to_owned());

            let block_inner = finished_block.inner(current_jobs_split_areas[i]);

            let [progress_area, second_area] = ratatui::layout::Layout::vertical([
                ratatui::layout::Constraint::Max(1),
                ratatui::layout::Constraint::Min(1),
            ])
            .areas(block_inner);

            frame.render_widget(finished_block, current_jobs_split_areas[i]);

            frame.render_widget(
                ratatui::widgets::Gauge::default()
                    .ratio(job.current_epoch as f64 / job.total_epochs as f64)
                    .label(format!("{}/{}", job.current_epoch, job.total_epochs))
                    .style(ratatui::style::Color::LightBlue)
                    .gauge_style(ratatui::style::Color::Blue),
                progress_area,
            );

            frame.render_widget(
                ratatui::widgets::List::new(
                    job.messages
                        .iter()
                        .cloned()
                        .rev()
                        .take(5)
                        .collect::<Vec<String>>(),
                ),
                second_area,
            );
        }

        frame.render_widget(
            ratatui::widgets::Paragraph::new(format!("{}|", self.current_input).as_str()),
            bottom_area,
        );
    }
}
