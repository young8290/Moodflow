//! Native peer validation lives here, outside the platform-independent Hub core.
use moodflow_hub_core::infra::imeconf::{ImeEvents, ImeRpc, ImeRpcError};

pub fn rpc() -> ImeRpc {
    let rpc = ImeRpc::from_env(cfg!(debug_assertions));
    #[cfg(windows)]
    let rpc = rpc.with_connection_check(moodflow_rpc::verify_server);
    rpc
}
pub fn events(endpoint: &str) -> Result<ImeEvents, ImeRpcError> {
    #[cfg(windows)]
    let check =
        Some(moodflow_rpc::verify_server as moodflow_hub_core::infra::imeconf::ConnectionCheck);
    #[cfg(not(windows))]
    let check = None;
    ImeEvents::connect_with_check(endpoint, check)
}
