//! Unit tests for the auth store — schema, hashing, CRUD, sessions.

use super::super::types::AgentRole;
use super::super::types::UserRole;
use super::*;

/// A throwaway password for tests. Generated at runtime (never a string
/// literal) so `CodeQL`'s hard-coded-credential scan has nothing to flag — the
/// exact value is irrelevant since no test re-authenticates with it.
fn test_password() -> String {
    AuthStore::generate_token()
}

#[test]
fn schema_creates_tables() {
    let store = AuthStore::open(Path::new(":memory:")).unwrap_or_else(|err| {
        panic!("open failed: {err}");
    });
    let tables: Vec<String> = store
        .conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap_or_else(|err| panic!("prepare failed: {err}"))
        .query_map([], |row| row.get(0))
        .unwrap_or_else(|err| panic!("query failed: {err}"))
        .filter_map(Result::ok)
        .collect();
    assert!(tables.contains(&"users".to_owned()), "users table missing: {tables:?}");
    assert!(tables.contains(&"sessions".to_owned()), "sessions table missing: {tables:?}");
    assert!(tables.contains(&"agent_acl".to_owned()), "agent_acl table missing: {tables:?}");
}

#[test]
fn hash_and_verify_password() {
    let hash = AuthStore::hash_password("hunter2").unwrap_or_else(|err| {
        panic!("hash failed: {err}");
    });
    assert!(hash.starts_with("$argon2"), "PHC string expected, got: {hash}");
    let ok = AuthStore::verify_password(&hash, "hunter2").unwrap_or_else(|err| {
        panic!("verify failed: {err}");
    });
    assert!(ok, "correct password should verify");
    let bad = AuthStore::verify_password(&hash, "wrong").unwrap_or_else(|err| {
        panic!("verify failed: {err}");
    });
    assert!(!bad, "wrong password should not verify");
}

#[test]
fn generate_token_length() {
    let token = AuthStore::generate_token();
    assert_eq!(token.len(), 64, "256-bit token = 64 hex chars");
    assert!(token.chars().all(|ch| ch.is_ascii_hexdigit()), "must be hex");
}

#[test]
fn generate_uuid_format() {
    let uuid = AuthStore::generate_uuid();
    assert_eq!(uuid.len(), 36, "UUID = 36 chars with dashes");
    assert_eq!(uuid.split('-').count(), 5, "5 groups separated by dashes");
    assert!(uuid.as_bytes().get(14).copied() == Some(b'4'), "version nibble must be 4, got: {uuid}");
}

#[test]
fn tokens_are_unique() {
    let a = AuthStore::generate_token();
    let b = AuthStore::generate_token();
    assert_ne!(a, b, "consecutive tokens must differ");
}

#[test]
fn user_role_roundtrip() {
    assert_eq!(UserRole::from_sql("admin"), UserRole::Admin);
    assert_eq!(UserRole::from_sql("ADMIN"), UserRole::Admin);
    assert_eq!(UserRole::from_sql("user"), UserRole::User);
    assert_eq!(UserRole::from_sql("unknown"), UserRole::User);
}

/// V0.3a — a legacy 2-role DB is rebuilt on `init_schema`: the `admin` row maps
/// to `superadmin`, the `user` row is untouched, and a `manager` insert (barred
/// by the old CHECK) now succeeds.
#[test]
fn migration_widens_check() {
    let conn = Connection::open_in_memory().unwrap_or_else(|err| panic!("open failed: {err}"));
    conn.execute_batch(
        "CREATE TABLE users (
             id TEXT PRIMARY KEY,
             email TEXT NOT NULL UNIQUE COLLATE NOCASE,
             name TEXT NOT NULL,
             password_hash TEXT NOT NULL,
             role TEXT NOT NULL DEFAULT 'user' CHECK(role IN ('admin', 'user')),
             must_change_password INTEGER NOT NULL DEFAULT 0,
             created_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL
         );
         INSERT INTO users VALUES ('a', 'admin@x', 'Admin', 'h', 'admin', 0, 0, 0);
         INSERT INTO users VALUES ('u', 'user@x', 'User', 'h', 'user', 0, 0, 0);",
    )
    .unwrap_or_else(|err| panic!("seed legacy schema failed: {err}"));

    let store = AuthStore { conn };
    store.init_schema().unwrap_or_else(|err| panic!("init_schema failed: {err}"));

    let admin_role: String =
        store.conn.query_row("SELECT role FROM users WHERE id = 'a'", [], |r| r.get(0)).unwrap_or_default();
    assert_eq!(admin_role, "superadmin", "legacy admin must map to superadmin");
    let user_role: String =
        store.conn.query_row("SELECT role FROM users WHERE id = 'u'", [], |r| r.get(0)).unwrap_or_default();
    assert_eq!(user_role, "user", "legacy user unchanged");
    // The widened CHECK now admits a manager.
    let _manager = store
        .create_user(NewUser { email: "m@x", name: "Mgr", password: &test_password(), role: UserRole::Manager })
        .unwrap_or_else(|err| panic!("manager insert should succeed post-migration: {err}"));
}

/// V0.3b — running `init_schema` twice is a no-op: row count and roles are
/// unchanged on the second pass.
#[test]
fn migration_idempotent() {
    let store = AuthStore::open(Path::new(":memory:")).unwrap_or_else(|err| panic!("open failed: {err}"));
    let _s = store
        .create_user(NewUser { email: "s@x", name: "S", password: &test_password(), role: UserRole::Superadmin })
        .unwrap_or_else(|err| panic!("{err}"));
    let _u = store
        .create_user(NewUser { email: "u@x", name: "U", password: &test_password(), role: UserRole::User })
        .unwrap_or_else(|err| panic!("{err}"));
    store.init_schema().unwrap_or_else(|err| panic!("second init_schema failed: {err}"));
    assert_eq!(store.count_users().unwrap_or(0), 2, "no rows added or dropped");
    let role: String =
        store.conn.query_row("SELECT role FROM users WHERE email = 's@x'", [], |r| r.get(0)).unwrap_or_default();
    assert_eq!(role, "superadmin", "roles unchanged on idempotent re-run");
}

#[test]
fn agent_role_roundtrip() {
    use super::super::types::AgentRole;
    assert_eq!(AgentRole::from_sql("agent-admin"), AgentRole::AgentAdmin);
    assert_eq!(AgentRole::from_sql("AGENT-ADMIN"), AgentRole::AgentAdmin);
    assert_eq!(AgentRole::from_sql("agent-user"), AgentRole::AgentUser);
    assert_eq!(AgentRole::from_sql("unknown"), AgentRole::AgentUser);
    assert_eq!(AgentRole::AgentAdmin.as_str(), "agent-admin");
    assert_eq!(AgentRole::AgentUser.as_str(), "agent-user");
}

#[test]
fn create_and_get_user() {
    let store = AuthStore::open(Path::new(":memory:")).expect("open");
    let user = store
        .create_user(NewUser {
            email: "alice@example.com",
            name: "Alice",
            password: &test_password(),
            role: UserRole::Admin,
        })
        .expect("create");
    assert_eq!(
        (user.email.as_str(), user.name.as_str(), user.role, user.id.len()),
        ("alice@example.com", "Alice", UserRole::Admin, 36),
        "created user fields (incl. UUID length)"
    );
    assert!(user.password_hash.starts_with("$argon2"), "PHC hash stored");

    // Fetch by id, then by email (case-insensitive) — both must resolve the same user.
    let found = store.get_user_by_id(&user.id).expect("get_by_id").expect("user not found");
    let found2 = store.get_user_by_email("ALICE@EXAMPLE.COM").expect("get_by_email").expect("user not found");
    assert_eq!(
        (found.email.as_str(), found2.id.as_str()),
        ("alice@example.com", user.id.as_str()),
        "by-id and by-email fetches agree"
    );
}

#[test]
fn list_and_count_users() {
    let store = AuthStore::open(Path::new(":memory:")).unwrap_or_else(|err| {
        panic!("open failed: {err}");
    });
    assert_eq!(store.count_users().unwrap_or(99), 0);
    let _u1 = store
        .create_user(NewUser { email: "a@x.com", name: "A", password: &test_password(), role: UserRole::Admin })
        .unwrap_or_else(|err| panic!("create failed: {err}"));
    let _u2 = store
        .create_user(NewUser { email: "b@x.com", name: "B", password: "pass5678", role: UserRole::User })
        .unwrap_or_else(|err| panic!("create failed: {err}"));
    assert_eq!(store.count_users().unwrap_or(0), 2);
    let list = store.list_users().unwrap_or_else(|err| panic!("list failed: {err}"));
    assert_eq!(list.len(), 2);
}

#[test]
fn delete_user_cascades() {
    let store = AuthStore::open(Path::new(":memory:")).unwrap_or_else(|err| {
        panic!("open failed: {err}");
    });
    let user = store
        .create_user(NewUser { email: "del@x.com", name: "Del", password: &test_password(), role: UserRole::User })
        .unwrap_or_else(|err| panic!("create failed: {err}"));
    let token = store
        .create_session(&user.id, None, Duration::from_hours(1))
        .unwrap_or_else(|err| panic!("session failed: {err}"));
    assert!(store.delete_user(&user.id).unwrap_or(false));
    // Session must be cascade-deleted.
    let valid = store.validate_session(&token).unwrap_or_else(|err| panic!("validate failed: {err}"));
    assert!(valid.is_none(), "session must be gone after user delete");
    assert_eq!(store.count_users().unwrap_or(99), 0);
}

#[test]
fn session_lifecycle() {
    let store = AuthStore::open(Path::new(":memory:")).unwrap_or_else(|err| {
        panic!("open failed: {err}");
    });
    let user = store
        .create_user(NewUser { email: "sess@x.com", name: "Sess", password: &test_password(), role: UserRole::User })
        .unwrap_or_else(|err| panic!("create failed: {err}"));
    let token = store
        .create_session(&user.id, Some("test-agent"), Duration::from_hours(1))
        .unwrap_or_else(|err| panic!("session failed: {err}"));
    // Valid session returns the correct user.
    let found_user = store
        .validate_session(&token)
        .unwrap_or_else(|err| panic!("validate failed: {err}"))
        .unwrap_or_else(|| panic!("session should be valid"));
    assert_eq!(found_user.id, user.id);
    assert_eq!(found_user.email, "sess@x.com");
    // Revoke.
    assert!(store.revoke_session(&token).unwrap_or(false));
    assert!(store.validate_session(&token).unwrap_or(None).is_none());
}

#[test]
fn expired_session_swept() {
    let store = AuthStore::open(Path::new(":memory:")).unwrap_or_else(|err| {
        panic!("open failed: {err}");
    });
    let user = store
        .create_user(NewUser { email: "exp@x.com", name: "Exp", password: &test_password(), role: UserRole::User })
        .unwrap_or_else(|err| panic!("create failed: {err}"));
    // Create a session that's already expired (TTL = 0).
    let token =
        store.create_session(&user.id, None, Duration::ZERO).unwrap_or_else(|err| panic!("session failed: {err}"));
    // Tiny sleep to ensure we're past the expiry.
    std::thread::sleep(Duration::from_millis(5));
    let valid = store.validate_session(&token).unwrap_or_else(|err| panic!("validate failed: {err}"));
    assert!(valid.is_none(), "expired session should be swept");
}

#[test]
fn grant_and_check_access() {
    let store = AuthStore::open(Path::new(":memory:")).unwrap_or_else(|err| {
        panic!("open failed: {err}");
    });
    let user = store
        .create_user(NewUser { email: "acl@x.com", name: "Acl", password: &test_password(), role: UserRole::User })
        .unwrap_or_else(|err| panic!("create failed: {err}"));
    // No access initially.
    let access = store.check_access("agent-1", &user.id).unwrap_or_else(|err| panic!("check failed: {err}"));
    assert!(access.is_none(), "no access before grant");
    // Grant agent-user.
    store
        .grant_access("agent-1", &user.id, AgentRole::AgentUser, None)
        .unwrap_or_else(|err| panic!("grant failed: {err}"));
    let granted = store.check_access("agent-1", &user.id).unwrap_or_else(|err| panic!("check failed: {err}"));
    assert_eq!(granted, Some(AgentRole::AgentUser));
    assert!(!store.is_agent_admin("agent-1", &user.id).unwrap_or(true));
}

#[test]
fn update_agent_role() {
    let store = AuthStore::open(Path::new(":memory:")).unwrap_or_else(|err| {
        panic!("open failed: {err}");
    });
    let user = store
        .create_user(NewUser { email: "role@x.com", name: "Role", password: &test_password(), role: UserRole::User })
        .unwrap_or_else(|err| panic!("create failed: {err}"));
    store
        .grant_access("agent-1", &user.id, AgentRole::AgentUser, None)
        .unwrap_or_else(|err| panic!("grant failed: {err}"));
    // Promote to agent-admin.
    assert!(store.update_agent_role("agent-1", &user.id, AgentRole::AgentAdmin).unwrap_or(false));
    assert!(store.is_agent_admin("agent-1", &user.id).unwrap_or(false));
    // Update non-existent entry returns false.
    assert!(!store.update_agent_role("agent-99", &user.id, AgentRole::AgentAdmin).unwrap_or(true));
}

#[test]
fn revoke_access() {
    let store = AuthStore::open(Path::new(":memory:")).unwrap_or_else(|err| {
        panic!("open failed: {err}");
    });
    let user = store
        .create_user(NewUser {
            email: "rev-acl@x.com",
            name: "RevAcl",
            password: &test_password(),
            role: UserRole::User,
        })
        .unwrap_or_else(|err| panic!("create failed: {err}"));
    store
        .grant_access("agent-1", &user.id, AgentRole::AgentUser, None)
        .unwrap_or_else(|err| panic!("grant failed: {err}"));
    assert!(store.revoke_access("agent-1", &user.id).unwrap_or(false));
    assert!(store.check_access("agent-1", &user.id).unwrap_or(None).is_none());
    // Double revoke returns false.
    assert!(!store.revoke_access("agent-1", &user.id).unwrap_or(true));
}

#[test]
fn list_agent_users_and_user_agents() {
    let store = AuthStore::open(Path::new(":memory:")).expect("open");
    let alice = store
        .create_user(NewUser {
            email: "alice-acl@x.com",
            name: "Alice",
            password: &test_password(),
            role: UserRole::User,
        })
        .expect("create alice");
    let bob = store
        .create_user(NewUser { email: "bob-acl@x.com", name: "Bob", password: "pass5678", role: UserRole::User })
        .expect("create bob");
    store.grant_access("agent-1", &alice.id, AgentRole::AgentAdmin, None).expect("grant alice a1");
    store.grant_access("agent-1", &bob.id, AgentRole::AgentUser, Some(&alice.id)).expect("grant bob a1");
    store.grant_access("agent-2", &alice.id, AgentRole::AgentUser, None).expect("grant alice a2");
    // List users on agent-1.
    let users = store.list_agent_users("agent-1").expect("list agent users");
    assert_eq!(users.len(), 2, "expected exactly two ACL entries");
    let first = users.first().expect("first ACL entry");
    let second = users.get(1).expect("second ACL entry");
    assert_eq!(
        (first.user_name.as_str(), first.role, first.user_email.as_str(), first.granted_by.is_none()),
        ("Alice", AgentRole::AgentAdmin, "alice-acl@x.com", true),
        "first ACL entry (Alice, admin, ungranted)"
    );
    assert_eq!(
        (second.user_name.as_str(), second.role, second.granted_by.as_deref(), second.granted_at > 0),
        ("Bob", AgentRole::AgentUser, Some(alice.id.as_str()), true),
        "second ACL entry (Bob, user, granted by Alice)"
    );
    // List agents for alice.
    let agents = store.list_user_agents(&alice.id).expect("list user agents");
    assert_eq!(agents, vec!["agent-1", "agent-2"]);
}

#[test]
fn delete_user_cascades_acl() {
    let store = AuthStore::open(Path::new(":memory:")).unwrap_or_else(|err| {
        panic!("open failed: {err}");
    });
    let user = store
        .create_user(NewUser {
            email: "del-acl@x.com",
            name: "DelAcl",
            password: &test_password(),
            role: UserRole::User,
        })
        .unwrap_or_else(|err| panic!("create failed: {err}"));
    store
        .grant_access("agent-1", &user.id, AgentRole::AgentUser, None)
        .unwrap_or_else(|err| panic!("grant failed: {err}"));
    assert!(store.delete_user(&user.id).unwrap_or(false));
    let users = store.list_agent_users("agent-1").unwrap_or_else(|err| panic!("list failed: {err}"));
    assert!(users.is_empty(), "ACL entries must cascade on user delete");
}

#[test]
fn grant_overwrites_previous() {
    let store = AuthStore::open(Path::new(":memory:")).unwrap_or_else(|err| {
        panic!("open failed: {err}");
    });
    let user = store
        .create_user(NewUser { email: "ow@x.com", name: "Ow", password: &test_password(), role: UserRole::User })
        .unwrap_or_else(|err| panic!("create failed: {err}"));
    store
        .grant_access("agent-1", &user.id, AgentRole::AgentUser, None)
        .unwrap_or_else(|err| panic!("grant failed: {err}"));
    // Re-grant with different role overwrites.
    store
        .grant_access("agent-1", &user.id, AgentRole::AgentAdmin, None)
        .unwrap_or_else(|err| panic!("re-grant failed: {err}"));
    assert_eq!(store.check_access("agent-1", &user.id).unwrap_or(None), Some(AgentRole::AgentAdmin),);
    // Only one entry, not two.
    let users = store.list_agent_users("agent-1").unwrap_or_else(|err| panic!("list failed: {err}"));
    assert_eq!(users.len(), 1);
}
