pub mod config;
pub mod setverifiedrole;
pub mod unverify;
pub mod userinfo;
mod utils;
pub mod verify;

use crate::bot::Error;
use serenity::all::{Command, Context};

/// Register all slash commands globally
pub async fn register_commands(ctx: &Context) -> Result<(), Error> {
    let commands = [
        verify::register(),
        unverify::register(),
        userinfo::register(),
        setverifiedrole::register(),
        config::register(),
    ];

    Command::set_global_commands(&ctx.http, &commands).await?;

    Ok(())
}
