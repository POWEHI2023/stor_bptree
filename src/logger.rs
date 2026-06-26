use std::sync::Once;

use tracing_subscriber::EnvFilter;

const DEFAULT_LOG_FILTER: &str = "stor_bptree=info";

static LOGGER_INIT: Once = Once::new();

pub fn init() {
    LOGGER_INIT.call_once(|| {
        dotenvy::dotenv().ok();

        let filter = EnvFilter::try_from_default_env()
            .or_else(|_| EnvFilter::try_new(DEFAULT_LOG_FILTER))
            .expect("default log filter must be valid");

        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .compact()
            .finish();

        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}
