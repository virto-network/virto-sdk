import { css } from "./utils.js";

export const globalStyles = await css`
  :host {
    --white: white;
    --whitesmoke: whitesmoke;
    --darkslategray: darkslategray;
    --lightgreen: lightgreen;
    --darkseagreen: darkseagreen;
    --black: #0000004D;
    /*From Figma*/
    --green: #24AF37;
    --whitish-green: #c6ebc7;
    --dark-green: #006B0A;
    --grey-green: rgb(173, 190, 173);
    --extra-light-green: #DDFBE0;
    /*Dialog background*/
    --gradient: linear-gradient(180deg, rgba(255, 255, 255, 0.5) 0%, rgba(255, 255, 255, 0.5) 100%), radial-gradient(84.04% 109.28% at 10.3% 12.14%, rgba(86, 201, 96, 0.5) 0%, rgba(198, 235, 199, 0) 98.5%);
    /*Fonts*/
    --font-primary: Outfit, sans-serif;
    --font-secondary: Plus Jakarta, sans-serif;
    /* Unused by now*/
    --color-accent-rgb: rgb(72, 61, 139);
    --color-text-alt: #446;
  }
`;