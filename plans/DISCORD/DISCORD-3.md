# DISCORD-3: Authentication & Token Management

**Status:** Not started
**blocked_by:** [DISCORD-2]
**unlocks:** [DISCORD-4, DISCORD-5, DISCORD-6, DISCORD-7, DISCORD-8, DISCORD-9, DISCORD-10]

## Scope

Implement secure bot token management using hyperforge:
1. Store bot tokens in hyperforge secrets
2. Retrieve tokens for API calls
3. Token validation
4. Support multiple bot configurations
5. CLI commands for token management

## Acceptance Criteria

- [ ] Bot tokens stored in hyperforge with namespace `discord:bot_token:{name}`
- [ ] Method to set/update bot token via CLI
- [ ] Method to retrieve bot token for API calls
- [ ] Method to test/validate bot token
- [ ] Support for default token (name: "default")
- [ ] Error messages when token is missing or invalid
- [ ] Integration with DiscordClient from DISCORD-2

## Implementation Details

### Token Storage Schema

Hyperforge secret paths:
```
discord:bot_token:default      # Default bot token
discord:bot_token:prod         # Production bot
discord:bot_token:dev          # Development bot
discord:config:default_token   # Name of the default token to use
```

### Authentication Module

```rust
// auth.rs

use crate::plexus::PlexusError;
use std::sync::Arc;

pub struct TokenManager {
    hyperforge_client: Arc<HyperforgeClient>,
}

impl TokenManager {
    pub fn new(hyperforge_client: Arc<HyperforgeClient>) -> Self {
        Self { hyperforge_client }
    }

    /// Get bot token by name (or default if name is None)
    pub async fn get_token(&self, name: Option<&str>) -> Result<String, PlexusError> {
        let token_name = match name {
            Some(n) => n,
            None => {
                // Try to get configured default, or use "default"
                self.get_default_token_name().await.unwrap_or("default")
            }
        };

        let secret_path = format!("discord:bot_token:{}", token_name);

        self.hyperforge_client
            .get_secret(&secret_path)
            .await
            .map_err(|_| PlexusError::ExecutionError(
                format!(
                    "Bot token '{}' not found in hyperforge. Set it with: \
                     synapse hyperforge secret set {} YOUR_TOKEN",
                    token_name, secret_path
                )
            ))
    }

    /// Set a bot token
    pub async fn set_token(&self, name: &str, token: String) -> Result<(), PlexusError> {
        let secret_path = format!("discord:bot_token:{}", name);

        self.hyperforge_client
            .set_secret(&secret_path, token)
            .await
            .map_err(|e| PlexusError::ExecutionError(
                format!("Failed to store token: {}", e)
            ))
    }

    /// Get the name of the default token
    async fn get_default_token_name(&self) -> Option<&str> {
        self.hyperforge_client
            .get_secret("discord:config:default_token")
            .await
            .ok()
            .map(|s| Box::leak(s.into_boxed_str()) as &str)
    }

    /// Set the default token name
    pub async fn set_default_token(&self, name: &str) -> Result<(), PlexusError> {
        self.hyperforge_client
            .set_secret("discord:config:default_token", name.to_string())
            .await
            .map_err(|e| PlexusError::ExecutionError(
                format!("Failed to set default token: {}", e)
            ))
    }

    /// List all available bot token names
    pub async fn list_tokens(&self) -> Result<Vec<String>, PlexusError> {
        self.hyperforge_client
            .list_secrets_by_prefix("discord:bot_token:")
            .await
            .map(|paths| {
                paths
                    .into_iter()
                    .filter_map(|p| {
                        p.strip_prefix("discord:bot_token:")
                            .map(|s| s.to_string())
                    })
                    .collect()
            })
            .map_err(|e| PlexusError::ExecutionError(
                format!("Failed to list tokens: {}", e)
            ))
    }

    /// Validate a token by making a test API call
    pub async fn validate_token(&self, token: &str) -> Result<BotInfo, DiscordError> {
        let client = DiscordClient::new();
        let response = client.get(token, "/users/@me").await?;

        let bot_info: BotInfo = response.json().await?;
        Ok(bot_info)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BotInfo {
    pub id: String,
    pub username: String,
    pub discriminator: String,
    pub bot: bool,
    pub verified: bool,
}
```

### Update Discord Activation

```rust
// activation.rs (additions)

use super::auth::TokenManager;

#[derive(Clone)]
pub struct Discord {
    client: Arc<DiscordClient>,
    token_manager: Arc<TokenManager>,
}

impl Discord {
    pub fn new(hyperforge_client: Arc<HyperforgeClient>) -> Self {
        Self {
            client: Arc::new(DiscordClient::new()),
            token_manager: Arc::new(TokenManager::new(hyperforge_client)),
        }
    }

    pub fn token_manager(&self) -> &TokenManager {
        &self.token_manager
    }
}

#[hub_macro::hub_methods(
    namespace = "discord",
    version = "1.0.0",
    description = "Discord server management and API access",
    hub
)]
impl Discord {
    // ... existing methods ...

    /// Set bot token for authentication
    #[hub_macro::hub_method(
        description = "Store a Discord bot token in hyperforge",
        params(
            name = "Token identifier (e.g., 'default', 'prod', 'dev')",
            token = "Discord bot token from https://discord.com/developers/applications",
            set_as_default = "Set this as the default token (optional)"
        )
    )]
    async fn auth_set(
        &self,
        name: String,
        token: String,
        set_as_default: Option<bool>,
    ) -> impl Stream<Item = DiscordEvent> + Send + 'static {
        let token_manager = self.token_manager.clone();

        stream! {
            // Validate token first
            match token_manager.validate_token(&token).await {
                Ok(bot_info) => {
                    yield DiscordEvent::TokenValidated {
                        bot_id: bot_info.id.clone(),
                        bot_name: bot_info.username.clone(),
                    };

                    // Store token
                    match token_manager.set_token(&name, token).await {
                        Ok(_) => {
                            yield DiscordEvent::TokenStored {
                                name: name.clone(),
                            };

                            // Set as default if requested
                            if set_as_default.unwrap_or(false) {
                                if let Err(e) = token_manager.set_default_token(&name).await {
                                    yield DiscordEvent::Error {
                                        message: format!("Failed to set as default: {}", e),
                                        code: None,
                                    };
                                } else {
                                    yield DiscordEvent::DefaultTokenSet {
                                        name,
                                    };
                                }
                            }
                        }
                        Err(e) => {
                            yield DiscordEvent::Error {
                                message: format!("Failed to store token: {}", e),
                                code: None,
                            };
                        }
                    }
                }
                Err(e) => {
                    yield DiscordEvent::Error {
                        message: format!("Invalid token: {}", e),
                        code: None,
                    };
                }
            }
        }
    }

    /// Get bot info for a stored token
    #[hub_macro::hub_method(
        description = "Get information about a stored bot token",
        params(name = "Token identifier (optional, uses default if not specified)")
    )]
    async fn auth_info(
        &self,
        name: Option<String>,
    ) -> impl Stream<Item = DiscordEvent> + Send + 'static {
        let token_manager = self.token_manager.clone();

        stream! {
            match token_manager.get_token(name.as_deref()).await {
                Ok(token) => {
                    match token_manager.validate_token(&token).await {
                        Ok(bot_info) => {
                            yield DiscordEvent::BotInfo {
                                id: bot_info.id,
                                username: bot_info.username,
                                discriminator: bot_info.discriminator,
                                verified: bot_info.verified,
                            };
                        }
                        Err(e) => {
                            yield DiscordEvent::Error {
                                message: format!("Failed to get bot info: {}", e),
                                code: None,
                            };
                        }
                    }
                }
                Err(e) => {
                    yield DiscordEvent::Error {
                        message: e.to_string(),
                        code: None,
                    };
                }
            }
        }
    }

    /// List all stored bot tokens
    #[hub_macro::hub_method(
        description = "List all stored Discord bot token names"
    )]
    async fn auth_list(&self) -> impl Stream<Item = DiscordEvent> + Send + 'static {
        let token_manager = self.token_manager.clone();

        stream! {
            match token_manager.list_tokens().await {
                Ok(tokens) => {
                    yield DiscordEvent::TokenList {
                        tokens,
                    };
                }
                Err(e) => {
                    yield DiscordEvent::Error {
                        message: format!("Failed to list tokens: {}", e),
                        code: None,
                    };
                }
            }
        }
    }
}
```

### Event Types

Update `types.rs` to include authentication events:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DiscordEvent {
    // ... existing variants ...

    TokenValidated {
        bot_id: String,
        bot_name: String,
    },
    TokenStored {
        name: String,
    },
    DefaultTokenSet {
        name: String,
    },
    BotInfo {
        id: String,
        username: String,
        discriminator: String,
        verified: bool,
    },
    TokenList {
        tokens: Vec<String>,
    },
}
```

## CLI Usage Examples

```bash
# Store a bot token
synapse discord auth-set \
  --name default \
  --token "YOUR_BOT_TOKEN_HERE" \
  --set-as-default true

# Store additional tokens
synapse discord auth-set \
  --name prod \
  --token "PROD_TOKEN_HERE"

synapse discord auth-set \
  --name dev \
  --token "DEV_TOKEN_HERE"

# List all stored tokens
synapse discord auth-list

# Get info about the default token
synapse discord auth-info

# Get info about a specific token
synapse discord auth-info --name prod
```

## Hyperforge Integration

The Discord plugin needs access to the Hyperforge client. This requires parent context injection:

```rust
// In builder.rs or initialization code

use crate::activations::discord::Discord;
use crate::activations::hyperforge::Hyperforge;

// Create hyperforge first
let hyperforge = Hyperforge::new(config).await?;
let hyperforge_client = hyperforge.client();

// Create discord with hyperforge access
let discord = Discord::new(hyperforge_client.clone());

let plexus = Plexus::new()
    .register(hyperforge)
    .register_hub(discord);
```

## Security Considerations

1. **Token Storage:**
   - Tokens are stored in hyperforge's encrypted storage
   - Never log or display tokens in plaintext
   - Use masked display in CLI output (e.g., "MTk4...KWs")

2. **Token Validation:**
   - Validate tokens before storing
   - Catch and report invalid/expired tokens
   - Don't store tokens that fail validation

3. **Error Messages:**
   - Don't include token in error messages
   - Provide helpful guidance for token setup

## Testing Strategy

1. **Unit Tests:**
   - Token path formatting
   - Default token resolution
   - Error handling

2. **Integration Tests:**
   - Store and retrieve tokens (with mock hyperforge)
   - Token validation with mock Discord API
   - List tokens functionality

3. **Manual Testing:**
   - Complete auth flow with real tokens
   - Verify encrypted storage in hyperforge
   - Test default token behavior

## Dependencies

No new dependencies required beyond DISCORD-2 and hyperforge integration.

## Notes

- This ticket enables authentication but doesn't implement any Discord operations
- Guild operations (DISCORD-4+) will use this token management
- Consider adding token rotation/expiry tracking in future
