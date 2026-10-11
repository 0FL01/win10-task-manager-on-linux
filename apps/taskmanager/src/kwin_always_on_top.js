/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 * `request` is supplied as JSON by the native worker.
 */
(function () {
    var searchTimer = new QTimer();
    var cleanupTimer = new QTimer();
    var deadline = Date.now() + 2000;
    var finished = false;

    function unload() {
        // Keep the JS-owned timer reachable through its connected closure until it fires.
        cleanupTimer.stop();
        callDBus("org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting",
                 "unloadScript", request.plugin);
    }

    // Also retire the script if the application exits before receiving its reply.
    cleanupTimer.singleShot = true;
    cleanupTimer.timeout.connect(unload);
    cleanupTimer.start(3000);

    function report(status, window) {
        if (finished) return;
        finished = true;
        searchTimer.stop();
        callDBus(request.destination, "/AlwaysOnTop",
                 "io.github.taskmanager.TaskManager.AlwaysOnTop", "Report",
                 request.plugin, status, window ? window.keepAbove : false,
                 !!window, window ? String(window.internalId) : "");
    }

    function attempt() {
        try {
            var windows = workspace.windowList();
            var matches = [];
            for (var i = 0; i < windows.length; ++i) {
                var window = windows[i];
                if (!window.deleted && !window.transient && window.normalWindow &&
                    window.pid === request.pid && window.resourceClass === request.appId) {
                    matches.push(window);
                }
            }
            if (matches.length > 1) {
                report("ambiguous", null);
            } else if (matches.length === 1) {
                var target = matches[0];
                target.keepAbove = request.enabled;
                report(target.keepAbove === request.enabled ? "ok" : "rejected", target);
            } else if (Date.now() >= deadline) {
                report("not-found", null);
            }
        } catch (error) {
            print("Task Manager always on top: " + error);
            report("failed", null);
        }
    }

    searchTimer.timeout.connect(attempt);
    searchTimer.start(100);
    attempt();
})();
