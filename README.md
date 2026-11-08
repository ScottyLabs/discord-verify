<div align="center">
  <h1>
    <pre>discord-verify</pre>
  </h1>
</div>

An Andrew ID Verification bot!

## Development

[Join the Community-Based Projects team through governance](https://git.cmu.dev/ScottyLabs/governance#joining-a-team) to read the development secrets, then install Lix and log in to OpenBao by following kennel's [local development](https://docs.kennel.scottylabs.org/guides/local-development.html#installing-nix) and [secrets](https://docs.kennel.scottylabs.org/guides/secrets.html#local-development) guides.

Create [a new Discord bot](https://discord.com/developers/applications) or use one of your current ones, and put its token in `.env`:

```bash
cp .env.example .env
```

Everything else resolves from OpenBao when you enter the dev shell. Start Valkey and the OAuth relay, then run the bot:

```bash
nix develop
pitchfork start --local
cargo run
```

## Data Model

```diff
# Guild Configuration
guild:{guild_id}:log_channel                  -> string (channel_id)
guild:{guild_id}:role:verified                -> string (role_id)
guild:{guild_id}:role:unverified              -> string (role_id)
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
discord:{discord_id}:keycloak                 -> string (keycloak_id)
discord:{discord_id}:verified_at              -> string (unix_timestamp)
keycloak:{keycloak_id}:discord                -> string (discord_id)

# Temporary Verification State (TTL: 10 minutes)
verify:{state_token}                          -> json (PendingVerification)
```
