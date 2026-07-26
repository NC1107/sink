import ReactDOM from "react-dom/client";
import App from "./App";
import { bootTheme } from "./store/theme";
import "material-symbols/outlined.css";
import "./styles/globals.css";

// Apply the saved theme before first paint to avoid a flash of the default.
bootTheme();

const root = document.getElementById("root");
if (root) {
  ReactDOM.createRoot(root).render(<App />);
}
