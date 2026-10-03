use std::fmt;
use std::str::FromStr;

use anyhow::{bail, Context, Result};
use solana_signature::Signature;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Source {
    Sig {
        signature: Signature,
        historical: bool,
    },
    Tx,
    From(i64),
    TxFrom(i64),
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::Sig {
                signature,
                historical: false,
            } => write!(f, "sig:{signature}"),
            Source::Sig {
                signature,
                historical: true,
            } => write!(f, "sig:{signature},historical"),
            Source::Tx => write!(f, "tx"),
            Source::From(id) => write!(f, "from:{id}"),
            Source::TxFrom(id) => write!(f, "tx,from:{id}"),
        }
    }
}

impl FromStr for Source {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if let Some(body) = text.strip_prefix("sig:") {
            let (raw, historical) = match body.split_once(',') {
                Some((raw, "historical")) => (raw, true),
                None => (body, false),
                Some(_) => bail!("invalid source {text}"),
            };
            return Ok(Self::Sig {
                signature: raw
                    .parse()
                    .with_context(|| format!("invalid source {text}"))?,
                historical,
            });
        }
        if let Some(raw) = text.strip_prefix("tx,from:") {
            return Ok(Self::TxFrom(run_id(text, raw)?));
        }
        if text == "tx" {
            return Ok(Self::Tx);
        }
        if let Some(raw) = text.strip_prefix("from:") {
            return Ok(Self::From(run_id(text, raw)?));
        }
        bail!("invalid source {text}")
    }
}

fn run_id(text: &str, raw: &str) -> Result<i64> {
    let id = raw
        .parse::<i64>()
        .with_context(|| format!("invalid source {text}"))?;
    if id.to_string() != raw {
        bail!("invalid source {text}");
    }
    Ok(id)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn round_trip() {
        let signature = Signature::from([1u8; 64]);
        let samples = [
            Source::Sig {
                signature,
                historical: false,
            },
            Source::Sig {
                signature,
                historical: true,
            },
            Source::Tx,
            Source::From(7),
            Source::TxFrom(7),
        ];
        for source in samples {
            let text = source.to_string();
            assert_eq!(text.parse::<Source>().unwrap(), source, "{text}");
        }
        for text in [
            "",
            "tx,from:",
            "from:01",
            "from:+7",
            "sig:nope",
            "sig:x,other",
        ] {
            assert!(text.parse::<Source>().is_err(), "{text}");
        }
    }
}
