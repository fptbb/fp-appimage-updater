use crate::commands::helpers::ExecutionContext;
use crate::self_updater;
use anyhow::Result;

pub fn run(ctx: &ExecutionContext, pre_release: bool) -> Result<()> {
    self_updater::self_update(ctx.client, pre_release, ctx.color_output)
}

pub fn run_if_available(ctx: &ExecutionContext, pre_release: bool) -> Result<()> {
    self_updater::self_update_if_available(ctx.client, pre_release, ctx.color_output)
}

pub fn check(ctx: &ExecutionContext, pre_release: bool) -> Result<()> {
    self_updater::check_for_update(ctx.client, pre_release, ctx.color_output)?;
    Ok(())
}
