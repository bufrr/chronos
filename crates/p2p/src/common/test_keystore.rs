use std::sync::Arc;

use futures::lock::Mutex;
use kitsune_p2p_types::dependencies::lair_keystore_api::{self, ipc_keystore_connect, LairClient, LairResult};
use tokio::runtime::Handle;
use tx5::deps::{hc_seed_bundle::PwHashLimits, sodoken};

/// Construct a new TestKeystore with the new lair api.
pub async fn spawn_test_keystore(passphrase: &[u8], connection_url: &str) -> LairResult<Arc<Mutex<LairClient>>> {
    let passphrase = passphrase.to_vec();
    let client = ipc_keystore_connect(connection_url.parse().unwrap(), passphrase)
        .await
        .unwrap();

    Ok(Arc::new(Mutex::new(client)))
}

pub fn test_keystore(passphrase: &[u8], connection_url: &str) -> Arc<Mutex<LairClient>> {
    let passphrase = passphrase.to_vec();
    tokio::task::block_in_place(move || {
        Handle::current().block_on(async { spawn_test_keystore(&passphrase, connection_url).await.unwrap() })
    })
}
