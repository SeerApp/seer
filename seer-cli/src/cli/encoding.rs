use anyhow::{Context, Result};
use base64::Engine;
use clap::Args;
use solana_transaction::versioned::VersionedTransaction;

use super::input::{PathOrValue, INPUT_HELP};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Encoding {
    Json,
    Wire,
    Base64,
    Base58,
    Hex,
}

impl Encoding {
    pub fn decode(self, bytes: &[u8]) -> Result<VersionedTransaction> {
        let payload = match self {
            Self::Json => {
                let text = std::str::from_utf8(bytes).context("transaction JSON utf-8")?;
                return serde_json::from_str::<solana_transaction::Transaction>(text.trim())
                    .map(VersionedTransaction::from)
                    .context("transaction JSON");
            }
            Self::Wire => bytes.to_vec(),
            Self::Hex => {
                let text = std::str::from_utf8(bytes).context("hex encoding utf-8")?;
                hex::decode(text.trim()).context("hex")?
            }
            Self::Base64 => {
                let text = std::str::from_utf8(bytes).context("base64 encoding utf-8")?;
                base64::engine::general_purpose::STANDARD
                    .decode(text.trim())
                    .context("base64")?
            }
            Self::Base58 => {
                let text = std::str::from_utf8(bytes).context("base58 encoding utf-8")?;
                bs58::decode(text.trim()).into_vec().context("base58")?
            }
        };
        bincode::deserialize(&payload).context("transaction wire")
    }
}

#[derive(Args, Debug)]
pub struct TxEncoding {
    #[arg(long, group = "encoding", help = "Input is base64-encoded")]
    b64: bool,
    #[arg(long, group = "encoding", help = "Input is base58-encoded")]
    b58: bool,
    #[arg(long, group = "encoding", help = "Input is hex-encoded")]
    hex: bool,
    #[arg(long, group = "encoding", help = "Input is Solana wire bytes")]
    wire: bool,
}

impl TxEncoding {
    pub fn decode(&self, tx: &PathOrValue) -> Result<VersionedTransaction> {
        let encoding = if self.b64 {
            Encoding::Base64
        } else if self.b58 {
            Encoding::Base58
        } else if self.hex {
            Encoding::Hex
        } else if self.wire {
            Encoding::Wire
        } else {
            Encoding::Json
        };
        encoding.decode(&tx.load_bytes()?)
    }
}

#[derive(Args, Debug)]
pub struct TxInput {
    #[command(flatten)]
    encoding: TxEncoding,
    #[arg(value_name = "TX", help = INPUT_HELP)]
    tx: PathOrValue,
}

impl TxInput {
    pub fn decode(&self) -> Result<VersionedTransaction> {
        self.encoding.decode(&self.tx)
    }
}
