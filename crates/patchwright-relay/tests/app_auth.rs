use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use patchwright_relay::{
    AppAuthenticator, GitHubAppConfiguration, GitHubAppError, KeyReference,
    ProtectedFileKeyProvider,
};
use serde::Deserialize;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use tempfile::tempdir;

#[derive(Debug, Deserialize)]
struct Claims {
    iat: i64,
    exp: i64,
    iss: String,
}

fn generate_rsa_key(root: &std::path::Path) -> (std::path::PathBuf, Vec<u8>) {
    let private = root.join("private.pem");
    let public = root.join("public.pem");
    assert!(
        Command::new("openssl")
            .args(["genrsa", "-out", private.to_str().unwrap(), "2048"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("openssl")
            .args([
                "rsa",
                "-in",
                private.to_str().unwrap(),
                "-pubout",
                "-out",
                public.to_str().unwrap(),
            ])
            .status()
            .unwrap()
            .success()
    );
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o600)).unwrap();
    (private, std::fs::read(public).unwrap())
}

#[test]
fn protected_key_reference_mints_bounded_redacted_app_jwt() {
    let root = tempdir().unwrap();
    let (private, public) = generate_rsa_key(root.path());
    let configuration = GitHubAppConfiguration::new(
        42,
        "Iv1.patchwright",
        KeyReference::protected_file(private.clone()),
        "https://api.github.com",
    )
    .unwrap();
    let authenticator =
        AppAuthenticator::new(configuration.clone(), ProtectedFileKeyProvider).unwrap();
    let now = 1_800_000_000;
    let jwt = authenticator.app_jwt(now).unwrap();
    let mut validation = Validation::new(Algorithm::RS256);
    validation.validate_exp = false;
    validation.set_required_spec_claims(&["iat", "exp", "iss"]);
    let claims = decode::<Claims>(
        jwt.expose_for_authorization_header(),
        &DecodingKey::from_rsa_pem(&public).unwrap(),
        &validation,
    )
    .unwrap()
    .claims;
    assert_eq!(claims.iss, "42");
    assert_eq!(claims.iat, now - 60);
    assert_eq!(claims.exp, now + 540);
    assert!(!format!("{configuration:?}{authenticator:?}{jwt:?}").contains("PRIVATE"));

    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        AppAuthenticator::new(configuration, ProtectedFileKeyProvider),
        Err(GitHubAppError::InsecureKeyFile)
    ));
}

#[test]
fn configuration_rejects_inline_invalid_and_non_https_key_boundaries() {
    assert!(KeyReference::parse("-----BEGIN PRIVATE KEY-----").is_err());
    assert!(KeyReference::parse("env:RAW_PRIVATE_KEY").is_err());
    assert!(
        GitHubAppConfiguration::new(
            0,
            "client",
            KeyReference::protected_file("/tmp/key"),
            "https://api.github.com"
        )
        .is_err()
    );
    assert!(
        GitHubAppConfiguration::new(
            1,
            "",
            KeyReference::protected_file("/tmp/key"),
            "https://api.github.com"
        )
        .is_err()
    );
    assert!(
        GitHubAppConfiguration::new(
            1,
            "client",
            KeyReference::protected_file("/tmp/key"),
            "http://api.github.com"
        )
        .is_err()
    );

    let root = tempdir().unwrap();
    let invalid = root.path().join("invalid.pem");
    std::fs::write(&invalid, "not a key").unwrap();
    std::fs::set_permissions(&invalid, std::fs::Permissions::from_mode(0o600)).unwrap();
    let configuration = GitHubAppConfiguration::new(
        1,
        "client",
        KeyReference::protected_file(invalid),
        "https://api.github.com",
    )
    .unwrap();
    let error = AppAuthenticator::new(configuration, ProtectedFileKeyProvider).unwrap_err();
    assert!(matches!(error, GitHubAppError::InvalidPrivateKey));
    assert!(!error.to_string().contains("not a key"));
}
