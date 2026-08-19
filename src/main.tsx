import ReactDOM from "react-dom/client";
import App from "./App";
import { MixPopout } from "./components/MixerBoard/MixPopout";
import { bootTheme } from "./store/theme";
import "material-symbols/outlined.css";
import "./styles/globals.css";

// Apply the saved theme before first paint to avoid a flash of the default.
bootTheme();

// A mix's popout window (see `open_mix_fader_window`) loads the same
// bundle with `?mixFader=<bus name>` - the query param decides which tree
// mounts.
const mixFader = new URLSearchParams(window.location.search).get("mixFader");

const root = document.getElementById("root");
if (root) {
  ReactDOM.createRoot(root).render(
    mixFader ? <MixPopout busName={mixFader} /> : <App />,
  );
}
