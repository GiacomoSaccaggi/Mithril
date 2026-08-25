# The Defenses of the Realm

> *"You cannot pass! I am a servant of the Secret Fire, wielder of the flame of Anor. The dark fire will not avail you, flame of Udûn!"* — Gandalf

Mithril implements multiple layers of security to protect your system while allowing productive AI-assisted development.

---

## Defense Overview

| Defense | Protects Against |
|---------|------------------|
| **Terminal Sanctuary** | Dangerous shell commands |
| **Path Traversal Guard** | Filesystem escape attempts |
| **Python Bypass Prevention** | Script-based attacks |
| **Argon2id Vaults** | Credential theft |
| **Retry Backoff** | Provider abuse and rate limits |
| **Mode Separation** | Unintended modifications |

---

## The Terminal Sanctuary

> *"There are older and fouler things than Orcs in the deep places of the world."*

The Terminal Sanctuary blocks dangerous commands that could harm your system.

### Blocked Commands

| Category | Commands |
|----------|----------|
| **System Destruction** | `rm -rf /`, `mkfs`, `dd if=/dev/zero` |
| **Privilege Escalation** | `sudo`, `su`, `chmod 777`, `chown root` |
| **Network Attacks** | `nc -e`, `bash -i >& /dev/tcp`, reverse shells |
| **Process Control** | `kill -9 1`, `killall`, system process termination |
| **Boot/Firmware** | `shutdown`, `reboot`, `halt`, BIOS modifications |

### Blocked Patterns

The sanctuary uses pattern matching to detect:

```rust
const BLOCKED_PATTERNS: &[&str] = &[
    r"rm\s+(-[rf]+\s+)*[/~]",           // Dangerous rm
    r">\s*/dev/sd",                      // Disk writes
    r"mkfs\.",                           // Filesystem creation
    r"dd\s+.*if=/dev/zero",              // Disk wiping
    r":\(\)\s*\{\s*:\|:\s*&\s*\};:",     // Fork bombs
    r"chmod\s+(-R\s+)?777\s+/",          // Dangerous permissions
    r"/dev/tcp/",                        // Reverse shells
    r"eval\s*\$\(",                      // Command injection
];
```

### Allowed Alternatives

| Blocked | Safe Alternative |
|---------|------------------|
| `rm -rf directory/` | `rm -r directory/` (prompts) |
| `sudo command` | Run mithril as needed user |
| `chmod 777` | `chmod 755` or more restrictive |

### Sanctuary Bypass

For legitimate needs, the sanctuary can be disabled per-session:

```bash
mithril chat --no-sanctuary  # Requires explicit flag
```

This flag is intentionally verbose and not persisted.

---

## Path Traversal Guard

> *"Short cuts make long delays."*

All file operations are validated to prevent escaping the allowed workspace.

### Validation Rules

1. **Resolve Symlinks** — All paths are canonicalized
2. **Check Ancestry** — Target must be under allowed roots
3. **Reject Escapes** — `../` sequences that escape are blocked
4. **Deny Absolutes** — Absolute paths outside workspace rejected

### Implementation

```rust
fn validate_path(path: &Path, workspace: &Path) -> Result<PathBuf> {
    let canonical = path.canonicalize()?;
    let workspace_canonical = workspace.canonicalize()?;
    
    if !canonical.starts_with(&workspace_canonical) {
        return Err(SecurityError::PathTraversal {
            attempted: path.to_path_buf(),
            workspace: workspace.to_path_buf(),
        });
    }
    
    Ok(canonical)
}
```

### Protected Paths

These paths are never accessible regardless of configuration:

| Path | Reason |
|------|--------|
| `/etc/passwd`, `/etc/shadow` | System credentials |
| `~/.ssh/` | SSH keys |
| `~/.gnupg/` | GPG keys |
| `~/.aws/credentials` | Cloud credentials |
| `/dev/*` | Device files |
| `/proc/*`, `/sys/*` | Kernel interfaces |

### Allowed Roots

By default, operations are restricted to:

1. Current working directory and below
2. `~/.mithril/` (configuration)
3. Explicitly configured additional paths

Configure additional roots in `~/.mithril/config.yaml`:

```yaml
security:
  allowed_paths:
    - /home/user/projects
    - /tmp/mithril-work
```

---

## Python Bypass Prevention

> *"Do not meddle in the affairs of wizards, for they are subtle and quick to anger."*

Agents cannot use Python or other scripting languages to bypass security controls.

### Blocked Script Invocations

| Pattern | Description |
|---------|-------------|
| `python -c "..."` | Inline Python code |
| `python script.py` | Script execution |
| `perl -e "..."` | Inline Perl |
| `ruby -e "..."` | Inline Ruby |
| `node -e "..."` | Inline JavaScript |
| `bash -c "..."` | Inline bash |

### Detection Method

The terminal operator scans commands for:

1. **Interpreter Names** — python, python3, perl, ruby, node, bash
2. **Inline Flags** — `-c`, `-e`, `--command`
3. **Script Extensions** — `.py`, `.pl`, `.rb`, `.js`, `.sh`
4. **Encoding Tricks** — base64, hex encoding in arguments

### Example Blocks

```bash
# All of these are blocked:
python -c "import os; os.remove('/')"
bash -c "rm -rf ~"
perl -e 'system("dangerous")'
echo "cm0gLXJmIH4=" | base64 -d | bash
```

---

## The Argon2id Vaults

> *"Keep it secret. Keep it safe."*

API keys and credentials are encrypted at rest using industry-standard cryptography.

### Encryption Stack

| Layer | Technology | Parameters |
|-------|------------|------------|
| **KDF** | Argon2id | m=64MB, t=3, p=4 |
| **Cipher** | AES-256-GCM | 256-bit key, 96-bit nonce |
| **Storage** | JSON + base64 | `~/.mithril/credentials.enc` |

### Key Derivation

```rust
let config = argon2::Config {
    variant: argon2::Variant::Argon2id,
    version: argon2::Version::Version13,
    mem_cost: 65536,      // 64 MB
    time_cost: 3,         // 3 iterations
    lanes: 4,             // Parallelism
    secret: &[],
    ad: &[],
    hash_length: 32,      // 256-bit key
};

let key = argon2::hash_raw(password, &salt, &config)?;
```

### Vault Structure

```json
{
  "version": 1,
  "salt": "base64-encoded-salt",
  "credentials": {
    "gemini": {
      "nonce": "base64-nonce",
      "ciphertext": "base64-encrypted-key"
    },
    "openai": {
      "nonce": "base64-nonce", 
      "ciphertext": "base64-encrypted-key"
    }
  }
}
```

### Password Handling

- **First Run** — Prompts for vault password, creates encrypted store
- **Subsequent** — Password cached in memory for session
- **Environment** — `MITHRIL_VAULT_PASSWORD` for automation (use with care)

### Credential Commands

```bash
mithril config set gemini "AIza..."    # Encrypted storage
mithril config get gemini               # Shows masked value
mithril config list                     # Shows key names only
```

---

## Retry with Exponential Backoff

> *"Despair is only for those who see the end beyond all doubt."*

Provider failures are handled gracefully with intelligent retry logic.

### Retry Strategy

| Attempt | Delay | Total Wait |
|---------|-------|------------|
| 1 | 0s | 0s |
| 2 | 1s | 1s |
| 3 | 2s | 3s |
| 4 | 4s | 7s |
| 5 | 8s | 15s |
| Max | 30s | - |

### Retryable Errors

| Error Type | Retried | Notes |
|------------|---------|-------|
| Connection timeout | ✅ | Network issues |
| 429 Too Many Requests | ✅ | Rate limiting |
| 500 Internal Server Error | ✅ | Provider issues |
| 502/503/504 | ✅ | Infrastructure |
| 401 Unauthorized | ❌ | Bad credentials |
| 400 Bad Request | ❌ | Invalid input |

### Configuration

```yaml
# ~/.mithril/config.yaml
providers:
  retry:
    max_attempts: 5
    initial_delay_ms: 1000
    max_delay_ms: 30000
    multiplier: 2.0
```

### Jitter

Random jitter (0-25%) is added to prevent thundering herd:

```rust
let jittered_delay = delay + (delay * random::<f64>() * 0.25);
```

---

## Mode Separation

> *"Many that live deserve death. And some that die deserve life."*

Plan and Build modes provide clear separation of intent.

### Plan Mode (Default)

| Capability | Allowed |
|------------|---------|
| Read files | ✅ |
| Search/grep | ✅ |
| Git status/log/diff | ✅ |
| Web search | ✅ |
| Write files | ❌ |
| Delete files | ❌ |
| Git commit | ❌ |
| Arbitrary terminal | ❌ |

### Build Mode

| Capability | Allowed |
|------------|---------|
| All Plan capabilities | ✅ |
| Write files | ✅ |
| Delete files | ✅ |
| Git commit | ✅ |
| Terminal (with sanctuary) | ✅ |

### Mode Indication

The TUI status bar shows current mode:
- 📖 **PLAN** — Blue indicator
- 🔨 **BUILD** — Orange indicator

Press `Tab` to toggle.

---

## Shadow Log Protection

> *"The Shadow that bred them can only mock, it cannot make."*

All file modifications are logged for recovery.

### Backup Triggers

| Operation | Backed Up |
|-----------|-----------|
| `write_file` | Original file (if exists) |
| `edit_file` | Original file |
| `delete_file` | Deleted file |
| `apply_patch` | All affected files |

### Log Structure

```
~/.mithril/shadow/
├── 2024-01-15T10-30-00/
│   ├── manifest.json
│   └── files/
│       ├── src__main.rs          # Path encoded
│       └── Cargo.toml
```

### Recovery

```bash
mithril undo                       # Restore last session
mithril undo --list                # Show all backups
mithril undo --session "2024-..."  # Restore specific
```

---

## Security Recommendations

### For Users

1. **Use Plan mode** by default for exploration
2. **Review before Build** — understand what will change
3. **Protect vault password** — don't commit to env files
4. **Regular undo review** — check shadow log periodically

### For Deployment

1. **Run unprivileged** — never as root
2. **Restrict paths** — configure minimal allowed_paths
3. **Network isolation** — bind to localhost only
4. **Audit logs** — enable debug logging for review

---

## Reporting Security Issues

Report vulnerabilities privately to security@mithril.dev — do not open public issues for security concerns.

---

> *"All shall love me and despair!"* — Just kidding, please use security responsibly.
