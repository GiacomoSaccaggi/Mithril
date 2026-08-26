# Config — The Vaults

Credential encryption (Argon2id + AES-256-GCM), provider settings, permission management.

## Key Concept

API keys are encrypted at rest. In Docker, env vars (MITHRIL_KEY_<PROVIDER>) take priority.

## Encryption Chain

1. Random salt generated
2. Argon2id derives encryption key
3. AES-256-GCM encrypts the API key
4. Encrypted blob + salt stored in config file

## Docker: Environment Variables

```bash
MITHRIL_KEY_GEMINI=AIza...
MITHRIL_KEY_OPENAI=sk-...
```

## Files

- `mod.rs` — Configuration management with Argon2id-derived AES-256-GCM credentials.
