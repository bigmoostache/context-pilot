//! First-boot account seeding.

use std::fmt;
use std::path::PathBuf;

use crate::resolve::Raw;

/// One account to create when the user table is empty.
///
/// Fields are private so the password can only leave through
/// [`Account::password`], which prefers the file form.
#[derive(Clone, PartialEq, Eq)]
pub struct Account {
    /// Login email.
    email: String,
    /// Display name.
    name: String,
    /// Inline initial password.
    password: Option<String>,
    /// File holding the initial password (wins over the inline form).
    password_file: Option<PathBuf>,
}

impl Account {
    /// The account described by `<prefix>_EMAIL` / `_NAME` / `_PASSWORD` /
    /// `_PASSWORD_FILE`, if the email is set.
    pub(crate) fn from_raw(raw: &Raw, prefix: &str) -> Option<Self> {
        let email = raw.text(&format!("{prefix}_EMAIL"))?.to_owned();
        Some(Self {
            email,
            name: raw.text(&format!("{prefix}_NAME")).unwrap_or_default().to_owned(),
            password: raw.text(&format!("{prefix}_PASSWORD")).map(str::to_owned),
            password_file: raw.path(&format!("{prefix}_PASSWORD_FILE")),
        })
    }

    /// Login email.
    #[must_use]
    pub fn email(&self) -> &str {
        &self.email
    }

    /// Display name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The initial password: the file's content (trailing newline stripped)
    /// when a non-empty file is configured, else the inline value.
    ///
    /// # Errors
    ///
    /// The file cannot be read, or neither form yields a non-empty password.
    pub fn password(&self) -> Result<String, String> {
        if let Some(path) = self.password_file.as_deref() {
            let text = std::fs::read_to_string(path).map_err(|err| format!("cannot read {}: {err}", path.display()))?;
            let trimmed = text.trim_end_matches(['\n', '\r']);
            if !trimmed.is_empty() {
                return Ok(trimmed.to_owned());
            }
        }
        self.password.clone().filter(|inline| !inline.is_empty()).ok_or_else(|| "no password configured".to_owned())
    }
}

impl fmt::Debug for Account {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Account")
            .field("email", &self.email)
            .field("name", &self.name)
            .field("password", &self.password.as_ref().map(|_secret| "[redacted]"))
            .field("password_file", &self.password_file)
            .finish()
    }
}

/// The two seedable accounts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seed {
    /// The vendor account.
    pub superadmin: Option<Account>,
    /// The client's top account.
    pub admin: Option<Account>,
}

impl Seed {
    /// Both accounts, each present when its email is set.
    pub(crate) fn from_raw(raw: &Raw) -> Self {
        Self {
            superadmin: Account::from_raw(raw, "CP_SEED_SUPERADMIN"),
            admin: Account::from_raw(raw, "CP_SEED_ADMIN"),
        }
    }
}
