#[tokio::main]
async fn main() -> std::process::ExitCode {
    ruvic::app::run().await.as_process_exit_code()
}
