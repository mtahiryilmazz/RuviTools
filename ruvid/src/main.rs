#[tokio::main]
async fn main() -> std::process::ExitCode {
    ruvid::app::run().await.as_process_exit_code()
}
