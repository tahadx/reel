.pragma library

// Reel's service and its bar widget share this file.
//
// Omarchy's stock bar gives a widget the live service object. A replacement bar
// (Station, for example) gives it a service-less facade instead: its
// bar.shell.serviceFor() always returns null. The service publishes a live
// reader and a closed action list here, and the widget reads that instead.
//
// Both entry points are loaded from the same plugin URL, so their imports of
// this file share one instance. Do not import it from a nested QML type: that
// creates a second library instance with no publisher.
//
// The pattern is the one Omarchy's replacement-bar plugins use for their own
// panels (Station's lib/BarBridge.js): a service-owned reader, a closed action
// allowlist, and no service QObject exposed to the bar.

var active = null

function publish(readSnapshot, actions) {
    var allowed = actions || ({})
    var api = Object.freeze({
        snapshot: function() {
            return active === api ? readSnapshot() : null
        },
        call: function(name, args) {
            if (active !== api) return undefined
            var key = String(name || "")
            if (!Object.prototype.hasOwnProperty.call(allowed, key)) return undefined
            if (typeof allowed[key] !== "function") return undefined
            return allowed[key].apply(null, Array.isArray(args) ? args : [])
        }
    })
    active = api
    return api
}

function current() { return active }

// A service retiring during a hot reload must not clear a newer publisher.
function clear(api) { if (active === api) active = null }

// The fields the panel consumes. The service owns the values; this only
// guards a malformed snapshot during a hot reload.
function snapshotValue(next) {
    var value = next || ({})
    return {
        state: value.state || null,
        recents: Array.isArray(value.recents) ? value.recents : [],
        elapsed: Number(value.elapsed) || 0,
        recentVersion: Number(value.recentVersion) || 0,
        outputDir: String(value.outputDir || ""),
        version: String(value.version || ""),
        updateInfo: value.updateInfo || null,
        updateChecking: value.updateChecking === true,
        lastError: String(value.lastError || ""),
        configureFailed: value.configureFailed === true
    }
}

