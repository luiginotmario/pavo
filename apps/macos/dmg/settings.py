# dmgbuild settings for Pavo.dmg. Icon positions match scripts/dmg-background.swift.
app = defines.get("app", "build/Pavo.app")  # noqa: F821 (dmgbuild provides `defines`)

format = "UDZO"
files = [app]
symlinks = {"Applications": "/Applications"}
hide_extensions = ["Pavo.app"]

background = "apps/macos/dmg/background.tiff"
window_rect = ((200, 140), (640, 440))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False

icon_size = 128
text_size = 13
icon_locations = {
    "Pavo.app": (170, 190),
    "Applications": (470, 190),
}
