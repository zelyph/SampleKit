// The version selector of the sidebar (_templates/sidebar/versions.html).
//
// The published site keeps one folder per version beside a versions.json,
// which tools/build-docs-version.sh writes:
//
//   versions.json   {"latest": "1.0.0", "versions": ["1.0.0", "1.0.0-rc.1"]}
//   latest/         a copy of the latest version
//   1.0.0/  1.0.0-rc.1/
//
// The page's version is the folder it is served from. Choosing another opens
// the same page there, or that version's front page when it has no such page.
// Without a versions.json — a site built alone, or opened from its folder —
// the selector stays hidden.
(function () {
  "use strict";

  function start() {
    var box = document.querySelector(".sk-versions");
    if (!box || !window.fetch || !window.URL) return;
    var select = box.querySelector(".sk-versions-select");
    var root = new URL(document.documentElement.dataset.content_root || "./", window.location.href);
    if (root.protocol !== "http:" && root.protocol !== "https:") return;
    var base = new URL("../", root);
    var folder = decodeURIComponent(root.pathname.replace(/\/$/, "").split("/").pop() || "");
    var page = window.location.href.slice(root.href.length).split("#")[0];

    fetch(new URL("versions.json", base), { cache: "no-cache" })
      .then(function (response) {
        if (!response.ok) throw new Error(String(response.status));
        return response.json();
      })
      .then(function (listing) {
        var versions = Array.isArray(listing.versions) ? listing.versions : [];
        var latest = typeof listing.latest === "string" ? listing.latest : null;
        var current = folder === "latest" ? latest : folder;
        if (versions.indexOf(current) < 0) return; // not served from a version folder

        versions.forEach(function (version) {
          var option = document.createElement("option");
          option.value = version;
          option.textContent = version === latest ? version + " (latest)" : version;
          option.selected = version === current;
          select.appendChild(option);
        });
        select.addEventListener("change", function () {
          go(base, select.value === latest ? "latest" : select.value, page);
        });

        if (latest && current !== latest) {
          var older = box.querySelector(".sk-versions-older");
          older.querySelector("a").href = new URL("latest/", base).href;
          older.hidden = false;
        }
        box.hidden = false;
      })
      .catch(function () {
        // No versions.json, or one that cannot be read: the site stands alone.
      });
  }

  // The same page in another version when it exists there; its front page otherwise.
  function go(base, target, page) {
    var same = new URL(target + "/" + page, base);
    var front = new URL(target + "/", base);
    fetch(same, { method: "HEAD" })
      .then(function (response) {
        window.location.href = (response.ok ? same : front).href;
      })
      .catch(function () {
        window.location.href = front.href;
      });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", start);
  } else {
    start();
  }
})();
