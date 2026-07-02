import { useMixerStore } from "../../store/mixer";
import { OutputSelect } from "../MixerBoard/OutputSelect";

/**
 * Master output: sends every channel to one device in a single action
 * (Sonar/Voicemeeter-style). The fast fix when a headset dies but its
 * dongle keeps the sink alive - one pick moves all audio to the speakers,
 * with no per-channel fiddling and no headset-specific detection.
 */
export function MasterOutput() {
  const channels = useMixerStore((s) => s.channels);
  const channelOutputs = useMixerStore((s) => s.channelOutputs);
  const setAllOutputs = useMixerStore((s) => s.setAllOutputs);

  if (channels.length === 0) return null;

  // The output shared by every channel, or "mixed" when they differ.
  const first = channelOutputs[channels[0].name] ?? null;
  const allSame = channels.every((c) => (channelOutputs[c.name] ?? null) === first);

  return (
    <OutputSelect
      value={allSame ? first : null}
      mixed={!allSame}
      onChange={(name) => void setAllOutputs(name)}
      title="Send every channel to one output device"
    />
  );
}
