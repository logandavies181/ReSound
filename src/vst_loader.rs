use std::{error::Error, path::Path};

use vst3_host::{PluginInfo, simple};

use crate::{
    luaapi::{InstrumentConfig, InstrumentType},
    plugin_output,
    renderer_input::VstInstrument,
};

pub fn resolve(
    instrument: &InstrumentConfig,
    project_dir: &Path,
) -> Result<VstInstrument, Box<dyn Error>> {
    if instrument.r#type != InstrumentType::Vst {
        return Err(format!("instrument '{}' is not a VST instrument", instrument.name).into());
    }
    let (plugin_path, _, _) =
        plugin_output::capture(|| select_plugin(simple::discover_plugins()?, &instrument.plugin))?;

    Ok(VstInstrument {
        name: instrument.name.clone(),
        plugin_path,
        preset_path: instrument.path.as_ref().map(|path| project_dir.join(path)),
        volume: instrument.volume,
    })
}

pub fn list_plugins() -> Result<Vec<String>, Box<dyn Error>> {
    let (plugins, _, _) = plugin_output::capture(|| Ok(simple::discover_plugins()?))?;
    Ok(plugins.into_iter().map(|plugin| plugin.name).collect())
}

pub fn show_plugin_parameters(plugin_name: &str) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let (parameters, _, _) = plugin_output::capture(|| {
        let plugin_path = select_plugin(simple::discover_plugins()?, plugin_name)?;
        let plugin = simple::load_plugin_with_settings(plugin_path, 44_100.0, 512)?;

        plugin
            .get_parameters()?
            .into_iter()
            .map(|parameter| {
                Ok((
                    parameter.name,
                    plugin.format_parameter(parameter.id, parameter.value)?,
                ))
            })
            .collect()
    })?;
    Ok(parameters)
}

fn select_plugin(
    plugins: impl IntoIterator<Item = PluginInfo>,
    name: &str,
) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let plugins = plugins.into_iter().collect::<Vec<_>>();
    plugins
        .iter()
        .find(|plugin| plugin.name == name)
        .or_else(|| {
            plugins
                .iter()
                .find(|plugin| plugin.path.file_stem().and_then(|stem| stem.to_str()) == Some(name))
        })
        .map(|plugin| plugin.path.clone())
        .ok_or_else(|| format!("VST3 plugin '{name}' was not found").into())
}

#[cfg(test)]
mod tests {
    use vst3_host::PluginInfo;

    use super::select_plugin;

    #[test]
    fn selects_plugin_by_configured_file_name() {
        let plugins = vec![
            plugin_info("Other", "/plugins/Other.vst3"),
            plugin_info("Configured Synth", "/plugins/synth.vst3"),
        ];

        assert_eq!(
            select_plugin(plugins, "synth").unwrap(),
            std::path::PathBuf::from("/plugins/synth.vst3")
        );
    }

    #[test]
    fn selects_plugin_by_discovered_name() {
        let plugins = vec![plugin_info("Configured Synth", "/plugins/plugin.vst3")];

        assert_eq!(
            select_plugin(plugins, "Configured Synth").unwrap(),
            std::path::PathBuf::from("/plugins/plugin.vst3")
        );
    }

    #[test]
    fn reports_when_configured_plugin_is_not_discovered() {
        assert_eq!(
            select_plugin(
                [plugin_info("Other", "/plugins/Other.vst3")],
                "Configured Synth"
            )
            .unwrap_err()
            .to_string(),
            "VST3 plugin 'Configured Synth' was not found"
        );
    }

    fn plugin_info(name: &str, path: &str) -> PluginInfo {
        PluginInfo {
            path: path.into(),
            name: name.into(),
            vendor: String::new(),
            version: String::new(),
            category: String::new(),
            uid: String::new(),
            audio_inputs: 0,
            audio_outputs: 0,
            has_midi_input: false,
            has_midi_output: false,
            has_gui: false,
        }
    }
}
