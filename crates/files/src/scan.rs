//! Malware scanning (FR-F02): a ClamAV daemon over TCP (`INSTREAM`), or, when no daemon is configured,
//! a built-in signature check for the EICAR test file only. The built-in scanner is for development and
//! E2E; production sets `--clamd` / `PROCERA_CLAMD_ADDR` (docs/06).

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Clean,
    Infected(String),
}

#[derive(Debug, Clone)]
pub enum Scanner {
    /// `host:port` of clamd.
    Clamd(String),
    /// Detects the EICAR test signature only (no daemon configured).
    EicarOnly,
}

const EICAR: &[u8] = br"X5O!P%@AP[4\PZX54(P^)7CC)7}$EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*";

impl Scanner {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Clamd(_) => "clamav",
            Self::EicarOnly => "built-in (EICAR test signature only)",
        }
    }

    pub async fn scan(&self, data: &[u8]) -> Result<Verdict, String> {
        match self {
            Self::EicarOnly => Ok(if data.windows(EICAR.len()).any(|w| w == EICAR) {
                Verdict::Infected("Eicar-Test-Signature".into())
            } else {
                Verdict::Clean
            }),
            Self::Clamd(addr) => {
                let mut s = TcpStream::connect(addr).await.map_err(|e| format!("clamd: {e}"))?;
                s.write_all(b"zINSTREAM\0").await.map_err(|e| format!("clamd: {e}"))?;
                for chunk in data.chunks(64 * 1024) {
                    s.write_all(&u32::try_from(chunk.len()).unwrap_or(0).to_be_bytes())
                        .await
                        .map_err(|e| format!("clamd: {e}"))?;
                    s.write_all(chunk).await.map_err(|e| format!("clamd: {e}"))?;
                }
                s.write_all(&0u32.to_be_bytes()).await.map_err(|e| format!("clamd: {e}"))?;
                let mut reply = String::new();
                s.read_to_string(&mut reply).await.map_err(|e| format!("clamd: {e}"))?;
                let reply = reply.trim_end_matches('\0').trim();
                if reply.ends_with("OK") {
                    Ok(Verdict::Clean)
                } else if let Some(sig) = reply.strip_suffix(" FOUND") {
                    Ok(Verdict::Infected(sig.rsplit(": ").next().unwrap_or(sig).to_owned()))
                } else {
                    Err(format!("clamd: {reply}"))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_builtin_scanner_flags_eicar_only() {
        let s = Scanner::EicarOnly;
        assert_eq!(s.scan(EICAR).await, Ok(Verdict::Infected("Eicar-Test-Signature".into())));
        assert_eq!(s.scan(b"a normal policy").await, Ok(Verdict::Clean));
    }
}
