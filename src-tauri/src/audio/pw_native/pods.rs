//! SPA pod helpers: building Props pods (volume/mute) and parsing them
//! back from node param events.

use std::io::Cursor;

use libspa::pod::deserialize::PodDeserializer;
use libspa::pod::serialize::PodSerializer;
use libspa::pod::{Object, Pod, Property, PropertyFlags, Value, ValueArray};
use pipewire::spa as libspa;

use crate::error::SinkError;

/// PulseAudio-compatible volume mapping: UI percent uses a cubic curve over
/// PipeWire's linear channelVolumes (what pactl/pavucontrol do).
pub fn percent_to_linear(percent: u8) -> f32 {
    let f = f32::from(percent) / 100.0;
    f * f * f
}

pub fn linear_to_percent(linear: f32) -> u8 {
    (linear.max(0.0).cbrt() * 100.0).round().clamp(0.0, 255.0) as u8
}

/// Serialize a Props object pod setting channel volumes and/or mute.
pub fn props_pod_bytes(
    volume_linear: Option<(f32, usize)>,
    mute: Option<bool>,
) -> Result<Vec<u8>, SinkError> {
    let mut properties = Vec::new();
    if let Some((linear, channels)) = volume_linear {
        properties.push(Property {
            key: libspa::sys::SPA_PROP_channelVolumes,
            flags: PropertyFlags::empty(),
            value: Value::ValueArray(ValueArray::Float(vec![linear; channels.max(1)])),
        });
    }
    if let Some(muted) = mute {
        properties.push(Property {
            key: libspa::sys::SPA_PROP_mute,
            flags: PropertyFlags::empty(),
            value: Value::Bool(muted),
        });
    }
    let object = Object {
        type_: libspa::sys::SPA_TYPE_OBJECT_Props,
        id: libspa::sys::SPA_PARAM_Props,
        properties,
    };
    let bytes = PodSerializer::serialize(Cursor::new(Vec::new()), &Value::Object(object))
        .map_err(|e| SinkError::Config(format!("pod serialize: {e:?}")))?
        .0
        .into_inner();
    Ok(bytes)
}

/// Extracted state from a Props param event.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct PropsState {
    /// Loudest channel volume, linear.
    pub volume_linear: Option<f32>,
    pub channels: Option<usize>,
    pub muted: Option<bool>,
}

/// Parse volume/mute out of a Props pod (best effort - missing or foreign
/// fields are ignored).
pub fn parse_props(pod: &Pod) -> PropsState {
    let mut state = PropsState::default();
    let Ok((_, Value::Object(object))) = PodDeserializer::deserialize_any_from(pod.as_bytes())
    else {
        return state;
    };
    if object.type_ != libspa::sys::SPA_TYPE_OBJECT_Props {
        return state;
    }
    for property in object.properties {
        if property.key == libspa::sys::SPA_PROP_channelVolumes {
            if let Value::ValueArray(ValueArray::Float(volumes)) = property.value {
                state.channels = Some(volumes.len());
                state.volume_linear = volumes
                    .into_iter()
                    .fold(None, |acc, v| Some(acc.map_or(v, |a: f32| a.max(v))));
            }
        } else if property.key == libspa::sys::SPA_PROP_mute {
            if let Value::Bool(muted) = property.value {
                state.muted = Some(muted);
            }
        }
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_curve_roundtrips() {
        for p in [0u8, 25, 50, 100, 150] {
            assert_eq!(linear_to_percent(percent_to_linear(p)), p);
        }
    }

    #[test]
    fn props_pod_roundtrips_through_parser() {
        let bytes = props_pod_bytes(Some((0.5, 2)), Some(true)).expect("serializes");
        let pod = Pod::from_bytes(&bytes).expect("valid pod");
        let state = parse_props(pod);
        assert_eq!(state.muted, Some(true));
        assert_eq!(state.channels, Some(2));
        assert!((state.volume_linear.expect("has volume") - 0.5).abs() < 1e-6);
    }
}
