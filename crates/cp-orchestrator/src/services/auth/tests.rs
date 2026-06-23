//! Unit tests for the auth store — schema, hashing, CRUD, sessions.

use super::*;

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
    let parts: Vec<&str> = uuid.split('-').collect();
    assert_eq!(parts.len(), 5, "5 groups separated by dashes");
    assert!(
        uuid.as_bytes().get(14).copied() == Some(b'4'),
        "version nibble must be 4, got: {uuid}"
    );
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
    let store = AuthStore::open(Path::new(":memory:")).unwrap_or_else(|err| {
        panic!("open failed: {err}");
    });
    let user = store
        .create_user("alice@example.com", "Alice", "password123", UserRole::Admin)
        .unwrap_or_else(|err| panic!("create failed: {err}"));
    assert_eq!(user.email, "alice@example.com");
    assert_eq!(user.name, "Alice");
    assert_eq!(user.role, UserRole::Admin);
    assert_eq!(user.id.len(), 36, "UUID format");
    assert!(user.password_hash.starts_with("$argon2"), "PHC hash stored");

    // Fetch by id.
    let found = store
        .get_user_by_id(&user.id)
        .unwrap_or_else(|err| panic!("get_by_id failed: {err}"))
        .unwrap_or_else(|| panic!("user not found"));
    assert_eq!(found.email, "alice@example.com");

    // Fetch by email (case-insensitive).
    let found2 = store
        .get_user_by_email("ALICE@EXAMPLE.COM")
        .unwrap_or_else(|err| panic!("get_by_email failed: {err}"))
        .unwrap_or_else(|| panic!("user not found"));
    assert_eq!(found2.id, user.id);
}

#[test]
fn list_and_count_users() {
    let store = AuthStore::open(Path::new(":memory:")).unwrap_or_else(|err| {
        panic!("open failed: {err}");
    });
    assert_eq!(store.count_users().unwrap_or(99), 0);
    let _u1 = store
        .create_user("a@x.com", "A", "pass1234", UserRole::Admin)
        .unwrap_or_else(|err| panic!("create failed: {err}"));
    let _u2 = store
        .create_user("b@x.com", "B", "pass5678", UserRole::User)
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
        .create_user("del@x.com", "Del", "pass1234", UserRole::User)
        .unwrap_or_else(|err| panic!("create failed: {err}"));
    let token = store
        .create_session(&user.id, None, Duration::from_secs(3600))
        .unwrap_or_else(|err| panic!("session failed: {err}"));
    assert!(store.delete_user(&user.id).unwrap_or(false));
    // Session must be cascade-deleted.
    let valid = store
        .validate_session(&token)
        .unwrap_or_else(|err| panic!("validate failed: {err}"));
    assert!(valid.is_none(), "session must be gone after user delete");
    assert_eq!(store.count_users().unwrap_or(99), 0);
}

#[test]
fn session_lifecycle() {
    let store = AuthStore::open(Path::new(":memory:")).unwrap_or_else(|err| {
        panic!("open failed: {err}");
    });
    let user = store
        .create_user("sess@x.com", "Sess", "pass1234", UserRole::User)
        .unwrap_or_else(|err| panic!("create failed: {err}"));
    let token = store
        .create_session(&user.id, Some("test-agent"), Duration::from_secs(3600))
        .unwrap_or_else(|err| panic!("session failed: {err}"));
    // Valid session.
    let (session, found_user) = store
        .validate_session(&token)
        .unwrap_or_else(|err| panic!("validate failed: {err}"))
        .unwrap_or_else(|| panic!("session should be valid"));
    assert_eq!(session.user_id, user.id);
    assert_eq!(found_user.email, "sess@x.com");
    assert_eq!(session.user_agent.as_deref(), Some("test-agent"));
    assert_eq!(session.token.len(), 64, "token is 256-bit hex");
    assert!(session.created_at > 0, "created_at is set");
    assert!(session.expires_at > session.created_at, "expires_at is after created_at");
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
        .create_user("exp@x.com", "Exp", "pass1234", UserRole::User)
        .unwrap_or_else(|err| panic!("create failed: {err}"));
    // Create a session that's already expired (TTL = 0).
    let token = store
        .create_session(&user.id, None, Duration::ZERO)
        .unwrap_or_else(|err| panic!("session failed: {err}"));
    // Tiny sleep to ensure we're past the expiry.
    std::thread::sleep(Duration::from_millis(5));
    let valid = store
        .validate_session(&token)
        .unwrap_or_else(|err| panic!("validate failed: {err}"));
    assert!(valid.is_none(), "expired session should be swept");
}

#[test]
fn revoke_all_sessions() {
    let store = AuthStore::open(Path::new(":memory:")).unwrap_or_else(|err| {
        panic!("open failed: {err}");
    });
    let user = store
        .create_user("rev@x.com", "Rev", "pass1234", UserRole::User)
        .unwrap_or_else(|err| panic!("create failed: {err}"));
    let _t1 = store.create_session(&user.id, None, Duration::from_secs(3600));
    let _t2 = store.create_session(&user.id, None, Duration::from_secs(3600));
    let revoked = store
        .revoke_all_sessions(&user.id)
        .unwrap_or_else(|err| panic!("revoke_all failed: {err}"));
    assert_eq!(revoked, 2);
}

#[test]
fn grant_and_check_access() {
    let store = AuthStore::open(Path::new(":memory:")).unwrap_or_else(|err| {
        panic!("open failed: {err}");
    });
    let user = store
        .create_user("acl@x.com", "Acl", "pass1234", UserRole::User)
        .unwrap_or_else(|err| panic!("create failed: {err}"));
    // No access initially.
    let access = store
        .check_access("agent-1", &user.id)
        .unwrap_or_else(|err| panic!("check failed: {err}"));
    assert!(access.is_none(), "no access before grant");
    // Grant agent-user.
    store
        .grant_access("agent-1", &user.id, AgentRole::AgentUser, None)
        .unwrap_or_else(|err| panic!("grant failed: {err}"));
    let access = store
        .check_access("agent-1", &user.id)
        .unwrap_or_else(|err| panic!("check failed: {err}"));
    assert_eq!(access, Some(AgentRole::AgentUser));
    assert!(!store.is_agent_admin("agent-1", &user.id).unwrap_or(true));
}

#[test]
fn update_agent_role() {
    let store = AuthStore::open(Path::new(":memory:")).unwrap_or_else(|err| {
        panic!("open failed: {err}");
    });
    let user = store
        .create_user("role@x.com", "Role", "pass1234", UserRole::User)
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
        .create_user("rev-acl@x.com", "RevAcl", "pass1234", UserRole::User)
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
    let store = AuthStore::open(Path::new(":memory:")).unwrap_or_else(|err| {
        panic!("open failed: {err}");
    });
    let alice = store
        .create_user("alice-acl@x.com", "Alice", "pass1234", UserRole::User)
        .unwrap_or_else(|err| panic!("create failed: {err}"));
    let bob = store
        .create_user("bob-acl@x.com", "Bob", "pass5678", UserRole::User)
        .unwrap_or_else(|err| panic!("create failed: {err}"));
    store
        .grant_access("agent-1", &alice.id, AgentRole::AgentAdmin, None)
        .unwrap_or_else(|err| panic!("grant failed: {err}"));
    store
        .grant_access("agent-1", &bob.id, AgentRole::AgentUser, Some(&alice.id))
        .unwrap_or_else(|err| panic!("grant failed: {err}"));
    store
        .grant_access("agent-2", &alice.id, AgentRole::AgentUser, None)
        .unwrap_or_else(|err| panic!("grant failed: {err}"));
    // List users on agent-1.
    let users = store
        .list_agent_users("agent-1")
        .unwrap_or_else(|err| panic!("list failed: {err}"));
    assert_eq!(users.len(), 2);
    assert_eq!(users[0].user_name, "Alice");
    assert_eq!(users[0].role, AgentRole::AgentAdmin);
    assert_eq!(users[0].user_email, "alice-acl@x.com");
    assert!(users[0].granted_by.is_none());
    assert_eq!(users[1].user_name, "Bob");
    assert_eq!(users[1].role, AgentRole::AgentUser);
    assert_eq!(users[1].granted_by.as_deref(), Some(alice.id.as_str()));
    assert!(users[1].granted_at > 0);
    // List agents for alice.
    let agents = store
        .list_user_agents(&alice.id)
        .unwrap_or_else(|err| panic!("list failed: {err}"));
    assert_eq!(agents, vec!["agent-1", "agent-2"]);
}

#[test]
fn auto_grant_creator() {
    let store = AuthStore::open(Path::new(":memory:")).unwrap_or_else(|err| {
        panic!("open failed: {err}");
    });
    let user = store
        .create_user("creator@x.com", "Creator", "pass1234", UserRole::User)
        .unwrap_or_else(|err| panic!("create failed: {err}"));
    store
        .auto_grant_creator("new-agent", &user.id)
        .unwrap_or_else(|err| panic!("auto_grant failed: {err}"));
    assert!(store.is_agent_admin("new-agent", &user.id).unwrap_or(false));
}

#[test]
fn delete_user_cascades_acl() {
    let store = AuthStore::open(Path::new(":memory:")).unwrap_or_else(|err| {
        panic!("open failed: {err}");
    });
    let user = store
        .create_user("del-acl@x.com", "DelAcl", "pass1234", UserRole::User)
        .unwrap_or_else(|err| panic!("create failed: {err}"));
    store
        .grant_access("agent-1", &user.id, AgentRole::AgentUser, None)
        .unwrap_or_else(|err| panic!("grant failed: {err}"));
    assert!(store.delete_user(&user.id).unwrap_or(false));
    let users = store
        .list_agent_users("agent-1")
        .unwrap_or_else(|err| panic!("list failed: {err}"));
    assert!(users.is_empty(), "ACL entries must cascade on user delete");
}

#[test]
fn grant_overwrites_previous() {
    let store = AuthStore::open(Path::new(":memory:")).unwrap_or_else(|err| {
        panic!("open failed: {err}");
    });
    let user = store
        .create_user("ow@x.com", "Ow", "pass1234", UserRole::User)
        .unwrap_or_else(|err| panic!("create failed: {err}"));
    store
        .grant_access("agent-1", &user.id, AgentRole::AgentUser, None)
        .unwrap_or_else(|err| panic!("grant failed: {err}"));
    // Re-grant with different role overwrites.
    store
        .grant_access("agent-1", &user.id, AgentRole::AgentAdmin, None)
        .unwrap_or_else(|err| panic!("re-grant failed: {err}"));
    assert_eq!(
        store.check_access("agent-1", &user.id).unwrap_or(None),
        Some(AgentRole::AgentAdmin),
    );
    // Only one entry, not two.
    let users = store
        .list_agent_users("agent-1")
        .unwrap_or_else(|err| panic!("list failed: {err}"));
    assert_eq!(users.len(), 1);
}
