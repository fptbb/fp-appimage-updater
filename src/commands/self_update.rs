use crate::self_updater;
use anyhow::Result;

pub fn run(
    client: &ureq::Agent,
    pre_release: bool,
    color_output: bool,
) -> Result<()> {
    self_updater::self_update(client, pre_release, color_output)
}
