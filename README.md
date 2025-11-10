# Discord Verify

## Data Model

```diff
# Guild Configuration
guild:{guild_id}:role:verified                -> string (role_id)
guild:{guild_id}:role:level:Undergrad         -> string (role_id)
guild:{guild_id}:role:level:Graduate          -> string (role_id)
guild:{guild_id}:role:class:First-Year        -> string (role_id)
guild:{guild_id}:role:class:Sophomore         -> string (role_id)
guild:{guild_id}:role:class:Junior            -> string (role_id)
guild:{guild_id}:role:class:Senior            -> string (role_id)
guild:{guild_id}:role:class:Fifth-Year Senior -> string (role_id)
guild:{guild_id}:role:class:Masters           -> string (role_id)
guild:{guild_id}:role:class:Doctoral          -> string (role_id)

# Role assignment mode
guild:{guild_id}:role_mode                    -> string ("none" | "levels" | "classes" | "custom")
guild:{guild_id}:custom_levels                -> set (enabled level names)
guild:{guild_id}:custom_classes               -> set (enabled class names)

# User Verification Mappings
discord:{discord_id}:keycloak                 -> string (keycloak_user_id)
discord:{discord_id}:verified_at              -> string (unix_timestamp)
keycloak:{keycloak_id}:discord                -> string (discord_id)

# Temporary Verification State (TTL: 10 minutes)
verify:{state_token}                          -> json (PendingVerification)
```
