# Wireframes

The nine key screens of Panoply, light mode. Each image is rendered from the
source in [wireframes/src/](wireframes/src/). Open any `.dc.html` file there in
a browser to see it live; `support.js` renders it without any build step.

To re-render the images after editing a source file (macOS, Chrome installed):

```bash
cd docs/wireframes
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new \
  --hide-scrollbars --force-device-scale-factor=2 --window-size=1280,800 \
  --screenshot="$PWD/01-main.png" "file://$PWD/src/Main.dc.html"
```

Sizes per screen are in `src/canvas.json`.

## The bar: one shortcut, every tool

**1. Command bar at rest.** Glass panel, suggested and recent tools, keyboard
first, "Everything runs on this Mac".

![Command bar at rest](wireframes/01-main.png)

**2. A file dropped on the bar.** Tools that fit this file, ranked. A face was
found, so passport photo comes first; the GPS data is flagged.

![A file dropped on the bar](wireframes/02-bardrop.png)

**3. Say what you want.** "make this under 50 kb" becomes the right tool with
options filled in and the result estimated before running.

![Say what you want](wireframes/03-barintent.png)

## The app: tools, home and search

**4. A tool: passport photo.** Live preview with head guides, country rule,
background, automatic checks, print sheet output.

![Passport photo tool](wireframes/04-tool.png)

**5. App window, home.** Every category with tool counts, drop anything, quick
actions, nearby devices, recent results.

![App window home](wireframes/05-home.png)

**6. Find anything.** Text inside screenshots and photos, plain-words queries
turned into filters.

![Find anything](wireframes/06-find.png)

## Devices, modules and the phone

**7. Share between devices.** Devices on your network, transfers, a QR code for
devices without Panoply, clipboard sync, borrow this Mac.

![Share between devices](wireframes/07-nearby.png)

**8. Modules, by the people.** Community modules with permission badges, and how
small a module manifest is.

![Modules](wireframes/08-modules.png)

**9. Phone.** Share sheet with Panoply actions, including running a heavy job on
your computer, and the passport camera with live checks.

![Phone](wireframes/09-phone.png)
