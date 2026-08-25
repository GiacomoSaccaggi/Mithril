# The Rangers — Operators

> *"All that is gold does not glitter, not all those who wander are lost."* — Bilbo Baggins

The Rangers patrol the boundaries between the Armory (tools) and the outside world. Each operator handles a domain with appropriate security measures.

---

## Operator Overview

| Ranger | Domain | Tools Served |
|--------|--------|--------------|
| **File** | Filesystem | `read_file`, `write_file`, `edit_file`, `delete_file`, `apply_patch`, `list_files`, `find_file`, `file_stats` |
| **Git** | Version Control | `git_status`, `git_log`, `git_diff`, `git_blame`, `git_branch`, `git_commit` |
| **Web** | Network | `web_search`, `fetch_page` |
| **Terminal** | Shell | `run_terminal` |
| **Lore** | Knowledge | `lore_write`, `lore_read` |
| **Session** | State | `share_session` |

---

## File Operator

> *"Do not meddle in the affairs of wizards, for they are subtle and quick to anger."*

The File Operator manages all filesystem interactions with strict security.

### Responsibilities

- Path validation and canonicalization
- Shadow log backups before modifications
- Encoding detection and conversion
- Directory creation as needed

### Security Measures

| Measure | Description |
|---------|-------------|
| **Path Traversal Guard** | Blocks `../` escapes |
| **Symlink Resolution** | Follows and validates symlinks |
| **Protected Paths** | Denies access to system files |
| **Workspace Restriction** | Limits access to allowed roots |

### Implementation

```rust
pub struct FileOperator {
    workspace: PathBuf,
    shadow_log: ShadowLog,
    allowed_paths: Vec<PathBuf>,
}

impl FileOperator {
    pub fn read(&self, path: &str) -> Result<String> {
        let validated = self.validate_path(path)?;
        std::fs::read_to_string(validated)
            .map_err(|e| OperatorError::Read(e))
    }
    
    pub fn write(&self, path: &str, content: &str) -> Result<()> {
        let validated = self.validate_path(path)?;
        
        // Backup existing file
        if validated.exists() {
            self.shadow_log.backup(&validated)?;
        }
        
        // Create parent directories
        if let Some(parent) = validated.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        std::fs::write(validated, content)
            .map_err(|e| OperatorError::Write(e))
    }
    
    fn validate_path(&self, path: &str) -> Result<PathBuf> {
        let path = PathBuf::from(path);
        let canonical = if path.is_absolute() {
            path.canonicalize()?
        } else {
            self.workspace.join(path).canonicalize()?
        };
        
        // Check against protected paths
        for protected in PROTECTED_PATHS {
            if canonical.starts_with(protected) {
                return Err(OperatorError::ProtectedPath(canonical));
            }
        }
        
        // Check against allowed roots
        let allowed = self.allowed_paths.iter()
            .any(|root| canonical.starts_with(root));
        
        if !allowed && !canonical.starts_with(&self.workspace) {
            return Err(OperatorError::PathTraversal(canonical));
        }
        
        Ok(canonical)
    }
}
```

### Configuration

```yaml
operators:
  file:
    # Additional allowed paths
    allowed_paths:
      - /home/user/projects
      - /tmp/mithril-work
    
    # Maximum file size for read (bytes)
    max_read_size: 10485760  # 10 MB
    
    # Shadow log retention (days)
    shadow_retention: 7
```

---

## Git Operator

> *"Even the smallest person can change the course of the future."*

The Git Operator provides safe access to version control operations.

### Responsibilities

- Repository detection and validation
- Safe git command execution
- Diff generation and parsing
- Branch management

### Security Measures

| Measure | Description |
|---------|-------------|
| **Repo Boundary** | Operations confined to repository |
| **Read-Only by Default** | Writes require Build mode |
| **No Force Operations** | `--force` flags blocked |
| **No Remote Push** | Push operations require explicit confirmation |

### Implementation

```rust
pub struct GitOperator {
    workspace: PathBuf,
}

impl GitOperator {
    pub fn status(&self, path: &str) -> Result<GitStatus> {
        let repo_root = self.find_repo_root(path)?;
        
        let output = Command::new("git")
            .args(["status", "--porcelain", "-b"])
            .current_dir(&repo_root)
            .output()?;
        
        GitStatus::parse(&output.stdout)
    }
    
    pub fn diff(&self, opts: DiffOptions) -> Result<String> {
        let repo_root = self.find_repo_root(&opts.path)?;
        
        let mut args = vec!["diff"];
        
        if opts.staged {
            args.push("--staged");
        }
        
        if let Some(ref commit) = opts.commit {
            args.push(commit);
        }
        
        let output = Command::new("git")
            .args(&args)
            .current_dir(&repo_root)
            .output()?;
        
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
    
    pub fn commit(&self, message: &str, files: &[String]) -> Result<String> {
        // Stage files
        for file in files {
            self.stage_file(file)?;
        }
        
        // Create commit
        let output = Command::new("git")
            .args(["commit", "-m", message])
            .output()?;
        
        // Parse commit hash from output
        self.parse_commit_hash(&output.stdout)
    }
    
    fn find_repo_root(&self, path: &str) -> Result<PathBuf> {
        let start = self.workspace.join(path);
        let mut current = start.as_path();
        
        loop {
            if current.join(".git").exists() {
                return Ok(current.to_path_buf());
            }
            
            current = current.parent()
                .ok_or(OperatorError::NotARepository)?;
        }
    }
}
```

### Blocked Operations

| Operation | Reason |
|-----------|--------|
| `git push --force` | Destructive to remote |
| `git reset --hard` | Destructive to local |
| `git clean -f` | Deletes untracked files |
| `git rebase -i` | Interactive, requires terminal |

---

## Web Operator

> *"Many that live deserve death. And some that die deserve life. Can you give it to them?"*

The Web Operator handles network requests safely.

### Responsibilities

- HTTP requests with timeout
- Content extraction and sanitization
- Search query execution
- Response size limiting

### Security Measures

| Measure | Description |
|---------|-------------|
| **Timeout** | Requests timeout after 30s |
| **Size Limit** | Response body capped at 5MB |
| **URL Validation** | Only HTTP(S) allowed |
| **No Internal Network** | localhost/private IPs blocked |

### Implementation

```rust
pub struct WebOperator {
    client: reqwest::Client,
    max_response_size: usize,
}

impl WebOperator {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("Mithril/0.1")
            .build()
            .unwrap();
        
        Self {
            client,
            max_response_size: 5 * 1024 * 1024,
        }
    }
    
    pub async fn fetch(&self, url: &str) -> Result<WebPage> {
        self.validate_url(url)?;
        
        let response = self.client.get(url).send().await?;
        
        let content_length = response.content_length().unwrap_or(0);
        if content_length > self.max_response_size as u64 {
            return Err(OperatorError::ResponseTooLarge);
        }
        
        let body = response.text().await?;
        
        // Extract readable content
        let content = self.extract_content(&body)?;
        
        Ok(WebPage { url: url.to_string(), content })
    }
    
    fn validate_url(&self, url: &str) -> Result<()> {
        let parsed = url::Url::parse(url)?;
        
        // Only HTTP(S)
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return Err(OperatorError::InvalidScheme);
        }
        
        // No internal network
        if let Some(host) = parsed.host_str() {
            if host == "localhost" || host.starts_with("127.") || host.starts_with("192.168.") {
                return Err(OperatorError::InternalNetwork);
            }
        }
        
        Ok(())
    }
}
```

### Search Providers

```yaml
operators:
  web:
    search_provider: duckduckgo  # or google, bing
    timeout_seconds: 30
    max_results: 10
```

---

## Terminal Operator

> *"You shall not pass!"*

The Terminal Operator executes shell commands within the Sanctuary.

### Responsibilities

- Command parsing and validation
- Sanctuary rule enforcement
- Process execution with timeout
- Output capture and formatting

### Security Measures

| Measure | Description |
|---------|-------------|
| **Sanctuary** | Dangerous commands blocked |
| **Timeout** | Commands killed after limit |
| **No Shell Expansion** | Commands run directly |
| **Output Limit** | stdout/stderr capped |

### Implementation

```rust
pub struct TerminalOperator {
    workspace: PathBuf,
    sanctuary: Sanctuary,
    timeout: Duration,
}

impl TerminalOperator {
    pub async fn run(&self, command: &str, working_dir: Option<&str>) -> Result<CommandOutput> {
        // Check sanctuary rules
        self.sanctuary.validate(command)?;
        
        let dir = match working_dir {
            Some(d) => self.workspace.join(d),
            None => self.workspace.clone(),
        };
        
        let output = tokio::time::timeout(
            self.timeout,
            Command::new("sh")
                .arg("-c")
                .arg(command)
                .current_dir(dir)
                .output()
        ).await??;
        
        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        })
    }
}
```

### Sanctuary Rules

See [SECURITY.md](SECURITY.md) for full sanctuary documentation.

---

## Lore Operator

> *"I sit beside the fire and think of people long ago."*

The Lore Operator manages project knowledge persistence.

### Responsibilities

- Key-value knowledge storage
- Tag-based organization
- Search and retrieval
- Cross-session persistence

### Implementation

```rust
pub struct LoreOperator {
    store_path: PathBuf,
}

impl LoreOperator {
    pub fn write(&self, key: &str, content: &str, tags: &[String]) -> Result<LoreEntry> {
        let entry = LoreEntry {
            id: Uuid::new_v4(),
            key: key.to_string(),
            content: content.to_string(),
            tags: tags.to_vec(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        
        self.persist(&entry)?;
        Ok(entry)
    }
    
    pub fn read(&self, query: LoreQuery) -> Result<Vec<LoreEntry>> {
        let entries = self.load_all()?;
        
        entries.into_iter()
            .filter(|e| self.matches_query(e, &query))
            .collect()
    }
}
```

### Storage Format

```
.mithril/lore/
├── index.json
└── entries/
    ├── architecture.md
    ├── conventions.md
    └── decisions.md
```

---

## Session Operator

> *"I will not say: do not weep; for not all tears are an evil."*

The Session Operator handles state management and handoff.

### Responsibilities

- Session serialization
- Share token generation
- Cross-interface handoff
- Cleanup and expiry

### Implementation

```rust
pub struct SessionOperator {
    sessions_path: PathBuf,
    share_tokens: HashMap<String, ShareToken>,
}

impl SessionOperator {
    pub fn share(&mut self, session_id: Uuid, target: Interface) -> Result<String> {
        let token = self.generate_token();
        
        let share = ShareToken {
            token: token.clone(),
            session_id,
            created_at: Utc::now(),
            expires_at: Utc::now() + Duration::minutes(10),
            target,
        };
        
        self.share_tokens.insert(token.clone(), share);
        
        Ok(token)
    }
    
    pub fn load_shared(&mut self, token: &str) -> Result<Session> {
        let share = self.share_tokens.get(token)
            .ok_or(OperatorError::InvalidToken)?;
        
        if share.expires_at < Utc::now() {
            self.share_tokens.remove(token);
            return Err(OperatorError::TokenExpired);
        }
        
        self.load_session(share.session_id)
    }
}
```

---

## Operator Configuration

```yaml
# ~/.mithril/config.yaml
operators:
  file:
    max_read_size: 10485760
    shadow_retention: 7
    
  git:
    allow_push: false
    allow_force: false
    
  web:
    timeout_seconds: 30
    max_response_size: 5242880
    search_provider: duckduckgo
    
  terminal:
    timeout_seconds: 30
    max_output_size: 1048576
    sanctuary_enabled: true
    
  lore:
    max_entry_size: 102400
    
  session:
    share_token_expiry_minutes: 10
    max_sessions: 100
```

---

> *"The Rangers have ever been friends to the Shire."*
