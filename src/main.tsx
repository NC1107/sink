import ReactDOM from "react-dom/client";
import App from "./App";
import { MixPopout } from "./components/MixerBoard/MixPopout";
import { bootTheme } from "./store/theme";
import "material-symbols/outlined.css";
import "./styles/globals.css";

// Apply the saved theme before first paint to avoid a flash of the default.
bootTheme();

// The mix popout window (open_mix_fader_window) loads this same bundle;
// ?mixFader=<bus name> picks which tree mounts.
const mixFader = new URLSearchParams(window.location.search).get("mixFader");

const root = document.getElementById("root");
if (root) {
  ReactDOM.createRoot(root).render(mixFader ? <MixPopout busName={mixFader} /> : <App />);
}
