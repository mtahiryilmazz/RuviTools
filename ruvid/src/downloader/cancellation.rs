use tokio_util::sync::CancellationToken;

pub fn spawn_ctrl_c_listener(token: CancellationToken) {
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            token.cancel();
        }
    });
}
