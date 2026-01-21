use std::sync::Once;

static INIT: Once = Once::new();

pub fn init_proxy_env() {
    INIT.call_once(|| {
        std::env::set_var("OPINION_ENV", "local");
        std::env::set_var("OPINION_PROXY_URL", "http://127.0.0.1:7890");
        std::env::set_var("all_proxy", "http://127.0.0.1:7890");
    });
}
