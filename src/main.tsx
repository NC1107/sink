import ReactDOM from "react-dom/client";
import App from "./App";
import { MixPopout } from "./components/MixerBoard/MixPopout";
import { bootTheme } from "./store/theme";
import "material-symbols/outlined.css";
import "./styles/globals.css";

// Apply the saved theme before first paint to avoid a flash of the default.
bootTheme();

const params = new URLSearchParams(window.location.search);

// The window runs opaque on WebKitGTK 2.54+ (see webkit.rs); flag it so the
// CSS squares the corners the transparent window rounded.
if (params.get("opaque") === "1") {
  document.documentElement.dataset.opaque = "1";
}

// The mix popout window (open_mix_fader_window) loads this same bundle;
// ?mixFader=<bus name> picks which tree mounts.
const mixFader = params.get("mixFader");

const root = document.getElementById("root");
if (root) {
  ReactDOM.createRoot(root).render(mixFader ? <MixPopout busName={mixFader} /> : <App />);
}
