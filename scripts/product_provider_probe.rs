//! Explicit operator entry point. Not registered as a Cargo target or test.
#[path = "../src/product_provider/mod.rs"]
mod product_provider;
use product_provider::*;
use std::{path::PathBuf, thread, time::Duration};
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 6 {
        return Err("Usage: product_provider_probe OP codex|claude ABS_EXECUTABLE PRIVATE_JOB_ROOT NORMAL_CREDENTIAL_HOME [JOB_ID [DISCLOSURE_DIGEST APPROVAL_REFERENCE]]\nOP: probe | prepare-synthetic | submit-synthetic\nOnly submit-synthetic invokes a model, and it requires explicit prior approval of the exact trusted-Harness disclosure, including possible host access. No command installs or logs in.".into());
    }
    let kind = match args[2].as_str() {
        "codex" => ProviderKind::Codex,
        "claude" => ProviderKind::Claude,
        _ => return Err("unknown provider".into()),
    };
    let transport = ProviderTransport::new(
        PathBuf::from(&args[4]),
        kind,
        PathBuf::from(&args[3]),
        PathBuf::from(&args[5]),
    )?;
    match args[1].as_str() {
        "probe" if args.len() == 6 => println!(
            "{}",
            serde_json::to_string_pretty(&transport.probe()).unwrap()
        ),
        "prepare-synthetic" if args.len() == 7 => {
            let request=ProviderRequest{request_id:args[6].clone(),provider:kind,source_digest:digest(b"synthetic transport probe v1"),prompt:b"Return the fictional greeting Hello, fictional workshop. No tools or other context are needed.".to_vec(),schema:br#"{"type":"object","properties":{"greeting":{"type":"string"}},"required":["greeting"],"additionalProperties":false}"#.to_vec(),purpose:"One synthetic structured transport probe; not product acceptance".into(),data_categories:vec!["fictional greeting".into(),"public JSON schema".into()],profile:CapabilityProfile::TrustedHarness,limits:JobLimits{timeout_ms:60_000,..JobLimits::default()}};
            let job = transport.prepare(request)?;
            println!("No model invoked. Review the complete disclosure before authorizing submission.\n{}\nDisclosure digest: {}",serde_json::to_string_pretty(&job.disclosure).unwrap(),job.disclosure.digest());
        }
        "submit-synthetic" if args.len() == 9 => {
            let receipt = transport.inspect(&args[6])?;
            if receipt.disclosure.purpose
                != "One synthetic structured transport probe; not product acceptance"
            {
                return Err("not a synthetic probe job".into());
            }
            let consent = ConsentReceipt {
                disclosure_digest: args[7].clone(),
                approval_reference: args[8].clone(),
                expires_at_unix_ms: unix_ms() + 60_000,
            };
            let prepared = PreparedJob {
                disclosure: receipt.disclosure,
                capabilities: receipt.capabilities,
            };
            let mut job = transport.submit(prepared, &consent)?;
            let receipt = loop {
                if let Some(receipt) = job.poll()? {
                    break receipt;
                }
                thread::sleep(Duration::from_millis(50));
            };
            println!("{}", serde_json::to_string_pretty(&receipt).unwrap());
            if receipt.state != JobState::TransportValidated {
                return Err("probe did not complete transport validation".into());
            }
            let result = transport
                .ingest(
                    &receipt.request_id,
                    &receipt.request_digest,
                    &receipt.source_digest,
                )?
                .ok_or("no validated transport payload")?;
            println!(
                "Untrusted final bytes: {}",
                String::from_utf8_lossy(&result.final_bytes)
            );
        }
        _ => return Err("invalid operation or argument count; no model invoked".into()),
    }
    Ok(())
}
