use tracing::Level;
use tracing_subscriber::EnvFilter;

static INIT_LOGGER: std::sync::Once = std::sync::Once::new();

pub fn init_logger_once() {
    INIT_LOGGER.call_once(|| {
        let env_filter = EnvFilter::from_default_env()
            .add_directive(Level::DEBUG.into())
            .add_directive("mp4parse=off".parse().unwrap());
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .init();
    });
}