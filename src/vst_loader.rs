use std::{error::Error, fs, path::Path};

use vst3_host::{PluginInfo, StateContext, simple};

use crate::{
    luaapi::{InstrumentConfig, InstrumentType},
    renderer_input::VstInstrument,
};

pub fn resolve(
    instrument: &InstrumentConfig,
    project_dir: &Path,
) -> Result<VstInstrument, Box<dyn Error>> {
    if instrument.r#type != InstrumentType::Vst {
        return Err(format!("instrument '{}' is not a VST instrument", instrument.name).into());
    }
    let plugin_name = plugin_name(instrument);
    let plugin_path = select_plugin(simple::discover_plugins()?, plugin_name)?;
    let preset_path = preset_path(instrument, project_dir)?;

    Ok(VstInstrument {
        name: instrument.name.clone(),
        plugin_path,
        preset_path,
        volume: instrument.volume,
    })
}

pub fn show_parameters(preset_path: &Path) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let plugin_name = preset_plugin_name(preset_path)
        .ok_or("show-parameters expects a .vital or .dexed preset file")?;
    let plugin_path = select_plugin(simple::discover_plugins()?, plugin_name)?;
    let mut plugin = simple::load_plugin_with_settings(plugin_path, 44_100.0, 512)?;
    let preset = fs::read(preset_path)?;
    plugin.load_state_with_context(&preset, &StateContext::preset_from_path(preset_path))?;

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
}

fn plugin_name(instrument: &InstrumentConfig) -> &str {
    preset_plugin_name(&instrument.path).unwrap_or(&instrument.name)
}

fn preset_plugin_name(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()? {
        "vital" => Some("Vital"),
        "dexed" => Some("Dexed"),
        _ => None,
    }
}

fn preset_path(
    instrument: &InstrumentConfig,
    project_dir: &Path,
) -> Result<std::path::PathBuf, Box<dyn Error>> {
    if preset_plugin_name(&instrument.path).is_some() {
        Ok(project_dir.join(&instrument.path))
    } else {
        Err(format!(
            "VST instrument '{}' must use a .vital or .dexed preset file",
            instrument.name
        )
        .into())
    }
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
    use crate::luaapi::{InstrumentConfig, InstrumentType};
    use vst3_host::PluginInfo;

    use super::{plugin_name, preset_path, select_plugin, show_parameters};

    #[test]
    fn show_parameters_rejects_unsupported_preset_files() {
        assert_eq!(
            show_parameters(std::path::Path::new("synth.vstpreset"))
                .unwrap_err()
                .to_string(),
            "show-parameters expects a .vital or .dexed preset file"
        );
    }

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

    #[test]
    fn vital_preset_selects_vital_and_uses_configured_path() {
        let instrument = InstrumentConfig {
            name: "my-patch".into(),
            path: "presets/my-patch.vital".into(),
            r#type: InstrumentType::Vst,
            volume: 1.0,
        };

        assert_eq!(plugin_name(&instrument), "Vital");
        assert_eq!(
            preset_path(&instrument, std::path::Path::new("/project")).unwrap(),
            std::path::PathBuf::from("/project/presets/my-patch.vital")
        );
    }

    #[test]
    fn dexed_preset_selects_dexed_and_uses_configured_path() {
        let instrument = InstrumentConfig {
            name: "my-patch".into(),
            path: "presets/my-patch.dexed".into(),
            r#type: InstrumentType::Vst,
            volume: 1.0,
        };

        assert_eq!(plugin_name(&instrument), "Dexed");
        assert_eq!(
            preset_path(&instrument, std::path::Path::new("/project")).unwrap(),
            std::path::PathBuf::from("/project/presets/my-patch.dexed")
        );
    }

    #[test]
    fn errors_when_vst_instrument_has_unsupported_preset_extension() {
        let instrument = InstrumentConfig {
            name: "synth".into(),
            path: "plugins/synth.vst3".into(),
            r#type: InstrumentType::Vst,
            volume: 1.0,
        };

        assert_eq!(
            preset_path(&instrument, std::path::Path::new("/project"))
                .unwrap_err()
                .to_string(),
            "VST instrument 'synth' must use a .vital or .dexed preset file"
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
