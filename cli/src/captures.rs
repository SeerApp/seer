use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use seer_protos_community_neoeinstein_prost::seer::captures::v1::{
    AccountState, GetCaptureRequest,
};
use seer_protos_community_neoeinstein_tonic::seer::captures::v1::tonic::captures_service_client::CapturesServiceClient;
use solana_account::Account;
use solana_pubkey::Pubkey;
use solana_signature::Signature;
use tonic::service::interceptor::InterceptedService;
use tonic::service::Interceptor;
use tonic::transport::{Channel, ClientTlsConfig};
use tonic::{Request, Status};

pub const CAPTURES_URL: &str = "https://tx.seer.run";

pub fn fetch_capture(
    endpoint: &str,
    signature: &Signature,
) -> Result<BTreeMap<Pubkey, Option<Account>>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("tokio")?;
    runtime.block_on(fetch(endpoint, signature))
}

async fn fetch(endpoint: &str, signature: &Signature) -> Result<BTreeMap<Pubkey, Option<Account>>> {
    let mut client = connect(endpoint).await?;
    let response = client
        .get_capture(GetCaptureRequest {
            signature: signature.to_string(),
        })
        .await
        .map_err(|status| capture_status(signature, status))?;
    let capture = response
        .into_inner()
        .capture
        .context("capture response is empty")?;
    let mut out = BTreeMap::new();
    for state in capture.pre_state {
        let (key, account) = captured_account(&state)?;
        if out.insert(key, account).is_some() {
            bail!("capture lists {key} twice");
        }
    }
    Ok(out)
}

fn captured_account(state: &AccountState) -> Result<(Pubkey, Option<Account>)> {
    let pubkey: [u8; 32] = state.pubkey.as_ref().try_into().context("account pubkey")?;
    let key = Pubkey::from(pubkey);
    let Some(account) = &state.account else {
        return Ok((key, None));
    };
    let owner: [u8; 32] = account
        .owner
        .as_ref()
        .try_into()
        .with_context(|| format!("owner of {key}"))?;
    Ok((
        key,
        Some(Account {
            lamports: account.lamports,
            data: account.data.to_vec(),
            owner: owner.into(),
            executable: account.executable,
            rent_epoch: account.rent_epoch,
        }),
    ))
}

fn capture_status(signature: &Signature, status: Status) -> anyhow::Error {
    if status.code() == tonic::Code::NotFound {
        anyhow::anyhow!("transaction {signature} was not captured")
    } else {
        anyhow::Error::from(status)
    }
}

struct Bearer {
    token: Option<tonic::metadata::MetadataValue<tonic::metadata::Ascii>>,
}

impl Interceptor for Bearer {
    fn call(&mut self, mut req: Request<()>) -> Result<Request<()>, Status> {
        if let Some(token) = &self.token {
            req.metadata_mut().insert("authorization", token.clone());
        }
        Ok(req)
    }
}

async fn connect(
    endpoint_url: &str,
) -> Result<CapturesServiceClient<InterceptedService<Channel, Bearer>>> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .ok();
    let mut endpoint = Channel::from_shared(endpoint_url.to_string()).context("captures url")?;
    if endpoint_url.starts_with("https://") {
        endpoint = endpoint
            .tls_config(ClientTlsConfig::new().with_native_roots())
            .context("captures tls")?;
    }
    let channel = endpoint.connect().await.context("connect to captures")?;
    let token = std::env::var("SEER_API_KEY")
        .ok()
        .filter(|key| !key.is_empty());
    let token = token
        .map(|key| format!("Bearer {key}").parse().context("SEER_API_KEY"))
        .transpose()?;
    Ok(CapturesServiceClient::with_interceptor(
        channel,
        Bearer { token },
    ))
}
