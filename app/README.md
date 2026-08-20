# Basecamp module

A read-only panel showing the live state of the three deployed Antumbra
programs, fetched from a LEZ sequencer over JSON-RPC and decoded from the borsh
account data. It holds no keys and signs nothing — giving an analytics surface
signing power would make it a custody risk for no gain.

`antumbra-lez.lgx` is the package. It carries one variant, `darwin-arm64`.

## It loads, and here is the control that makes that mean something

Building a plugin is not loading it, and loading it is not implementing the
host's interface. All three were checked against **Basecamp 0.2.2's own bundled
Qt**, not against ours:

```
  Qt in this process : 6.9.2
  declared IID       : com.logos.component.IComponent
  load()             : ok
  instance()         : AntumbraPlugin
  qobject_cast       : IComponent obtained
```

And the same source, built against Homebrew's Qt instead:

```
  load()             : FAILED - The plugin uses incompatible Qt library. (6.11.0) [release]
```

That second run is the point. **Qt's version check is a ceiling, not a floor**:
it refuses any plugin whose minor version exceeds the host's. Basecamp 0.2.2
bundles Qt 6.9.2, so a plugin built against the current Homebrew Qt is rejected
outright — and rejected *silently* from the user's side, because Basecamp's file
log truncates before the loader messages. A tile appears, clicking it produces
nothing, and there is no visible error.

The dylib **extracted from the packaged `.lgx`** was load-tested too, not just
the one in `build/`. A package that ships a different binary from the one you
verified has verified nothing.

## Build it

Basecamp bundles Qt 6.9.2, so build against 6.9.2. Get it without disturbing the
system Qt:

```bash
python3 -m venv /tmp/aqt && /tmp/aqt/bin/pip install aqtinstall
/tmp/aqt/bin/aqt install-qt mac desktop 6.9.2 clang_64 --outputdir /tmp/Qt

cd app
cmake -S . -B build -DCMAKE_PREFIX_PATH=/tmp/Qt/6.9.2/macos -DCMAKE_BUILD_TYPE=Release
cmake --build build -j4
```

Official builds also reference frameworks as `@rpath/…`, which resolves against
Basecamp's bundled Qt; Homebrew builds hardcode `/opt/homebrew/opt/qtbase/lib/…`
and fail anywhere but this machine.

Then package:

```bash
python3 scripts/package-lgx.py --out app/antumbra-lez.lgx
python3 scripts/package-lgx.py --verify app/antumbra-lez.lgx
```

## Three things that are not obvious and cost a build each

1. **`QtConcurrent` is not bundled by Basecamp.** Linking it makes the plugin
   unloadable. The bridge is asynchronous through `QNetworkAccessManager`
   instead, which also avoids freezing the host with a nested event loop inside
   `createWidget`.
2. **The IID must be `com.logos.component.IComponent`.** That exact string is
   what `qobject_cast` compares across the plugin boundary; a private one gives
   *"Plugin does not implement IComponent"*.
3. **The `IComponent` vtable is exactly three entries** — the destructor,
   `createWidget`, `destroyWidget`. An extra virtual shifts every later slot, so
   the host calls the wrong function through a pointer that cast fine. `name()`
   is deliberately a non-virtual accessor.

And a fourth from the manifest: `lgx add` leaves `type` empty because it never
reads `metadata.json`, and **a module with an empty `type` is invisible** in
Basecamp. `package-lgx.py` folds it back in, which is why the manifest here
reads `type: ui`.
