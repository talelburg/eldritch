//! The allowlist: each entry is an [`Account`], matched by any of its emails
//! (ADR 0023).
//!
//! The host edits it over ssh with `server accounts`, which calls [`add`],
//! [`remove`] and [`list`]. An email is held as an [`Email`], trimmed and
//! lowercased, so the uniqueness check, `remove` and a sign-in's lookup agree
//! whatever case or padding it was typed or reported in.

use std::collections::BTreeMap;
use std::fmt;

use sqlx::SqlitePool;

/// An account's stable id: the allowlist entry's primary key, never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AccountId(pub i64);

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// An email address as the allowlist holds it: trimmed and lowercased.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Email(String);

impl Email {
    /// Read an email as typed or reported, trimming and lowercasing it.
    ///
    /// # Errors
    ///
    /// [`AccountsError::InvalidEmail`] when it holds no `@`.
    pub fn parse(raw: &str) -> Result<Self, AccountsError> {
        let email = raw.trim().to_lowercase();
        if email.contains('@') {
            Ok(Self(email))
        } else {
            Err(AccountsError::InvalidEmail(email))
        }
    }

    /// The normalised address.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Email {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// One allowlist entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// The entry's stable id.
    pub id: AccountId,
    /// The name the host set, shown at the table.
    pub display_name: String,
    /// Every email that signs in as this account, sorted.
    pub emails: Vec<Email>,
    /// Set by [`remove`]: a disabled account is kept for the games that refer
    /// to it, but nobody signs in as it.
    pub disabled: bool,
}

/// Which entry `remove` disables: a string holding `@` names an email, and
/// anything else an [`AccountId`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryRef {
    /// The entry holding this email.
    Email(Email),
    /// The entry with this id.
    Id(AccountId),
}

impl EntryRef {
    /// Read what the host typed.
    ///
    /// # Errors
    ///
    /// [`AccountsError::InvalidEntry`] when it holds no `@` and isn't an id.
    pub fn parse(entry: &str) -> Result<Self, AccountsError> {
        if entry.contains('@') {
            Email::parse(entry).map(Self::Email)
        } else {
            entry
                .parse()
                .map(|id| Self::Id(AccountId(id)))
                .map_err(|_| AccountsError::InvalidEntry(entry.to_string()))
        }
    }
}

/// Why an allowlist edit was refused.
#[derive(Debug, thiserror::Error)]
pub enum AccountsError {
    /// The display name is empty or only whitespace.
    #[error("the display name is empty")]
    EmptyName,
    /// An email to add holds no `@`.
    #[error("{0:?} is not an email address")]
    InvalidEmail(String),
    /// An email to add is already on another entry.
    #[error("{email} is already on entry {holder} ({holder_name})")]
    EmailTaken {
        /// The email.
        email: Email,
        /// The entry holding it.
        holder: AccountId,
        /// That entry's display name.
        holder_name: String,
    },
    /// What `remove` was given is neither an email nor an id.
    #[error("{0:?} is neither an email nor an account id")]
    InvalidEntry(String),
    /// No entry matches what `remove` was given.
    #[error("no entry matches {0}")]
    NoSuchEntry(String),
    /// The database failed.
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// Add an entry named `display_name` holding `emails`, and return it.
///
/// The emails are read as [`Email`]s, and one typed twice is held once. The whole
/// entry is refused if any of them is on another entry already.
///
/// # Errors
///
/// [`AccountsError::EmptyName`], [`AccountsError::InvalidEmail`],
/// [`AccountsError::EmailTaken`] naming the entry that holds it, or
/// [`AccountsError::Database`].
pub async fn add(
    db: &SqlitePool,
    display_name: &str,
    emails: &[String],
) -> Result<Account, AccountsError> {
    let display_name = display_name.trim();
    if display_name.is_empty() {
        return Err(AccountsError::EmptyName);
    }
    let mut emails = emails
        .iter()
        .map(|email| Email::parse(email))
        .collect::<Result<Vec<_>, _>>()?;
    emails.sort();
    emails.dedup();

    let mut tx = db.begin().await?;
    for email in &emails {
        let holder: Option<(i64, String)> = sqlx::query_as(
            "SELECT accounts.account_id, accounts.display_name
             FROM account_emails JOIN accounts USING (account_id)
             WHERE account_emails.email = ?",
        )
        .bind(email.as_str())
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((holder, holder_name)) = holder {
            return Err(AccountsError::EmailTaken {
                email: email.clone(),
                holder: AccountId(holder),
                holder_name,
            });
        }
    }
    let (id,): (i64,) =
        sqlx::query_as("INSERT INTO accounts (display_name) VALUES (?) RETURNING account_id")
            .bind(display_name)
            .fetch_one(&mut *tx)
            .await?;
    for email in &emails {
        sqlx::query("INSERT INTO account_emails (email, account_id) VALUES (?, ?)")
            .bind(email.as_str())
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;

    Ok(Account {
        id: AccountId(id),
        display_name: display_name.to_string(),
        emails,
        disabled: false,
    })
}

/// Disable the entry `entry` names, end its sessions, and return its id.
/// Disabling one already disabled succeeds.
///
/// A connection already open as the account stays open: sign-in is checked
/// on each request and each connect, so it is the next of those that fails.
///
/// # Errors
///
/// [`AccountsError::NoSuchEntry`] when no entry matches, or
/// [`AccountsError::Database`].
pub async fn remove(db: &SqlitePool, entry: &EntryRef) -> Result<AccountId, AccountsError> {
    let mut tx = db.begin().await?;
    let id: Option<(i64,)> = match entry {
        EntryRef::Email(email) => {
            sqlx::query_as("SELECT account_id FROM account_emails WHERE email = ?")
                .bind(email.as_str())
                .fetch_optional(&mut *tx)
                .await?
        }
        EntryRef::Id(AccountId(id)) => {
            sqlx::query_as("SELECT account_id FROM accounts WHERE account_id = ?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
        }
    };
    let Some((id,)) = id else {
        return Err(AccountsError::NoSuchEntry(match entry {
            EntryRef::Email(email) => email.to_string(),
            EntryRef::Id(id) => id.to_string(),
        }));
    };
    sqlx::query("UPDATE accounts SET disabled = 1 WHERE account_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE account_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(AccountId(id))
}

/// Every entry, disabled ones included, in id order.
///
/// # Errors
///
/// [`AccountsError::Database`].
pub async fn list(db: &SqlitePool) -> Result<Vec<Account>, AccountsError> {
    let rows: Vec<(i64, String, bool)> =
        sqlx::query_as("SELECT account_id, display_name, disabled FROM accounts")
            .fetch_all(db)
            .await?;
    let mut accounts: BTreeMap<i64, Account> = rows
        .into_iter()
        .map(|(id, display_name, disabled)| {
            let account = Account {
                id: AccountId(id),
                display_name,
                emails: Vec::new(),
                disabled,
            };
            (id, account)
        })
        .collect();
    let emails: Vec<(i64, String)> =
        sqlx::query_as("SELECT account_id, email FROM account_emails ORDER BY email")
            .fetch_all(db)
            .await?;
    for (id, email) in emails {
        if let Some(account) = accounts.get_mut(&id) {
            account.emails.push(Email(email));
        }
    }
    Ok(accounts.into_values().collect())
}
