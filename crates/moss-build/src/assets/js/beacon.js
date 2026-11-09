// The visit counter of a site published to the moss host. The build writes the
// site's beacon address into `endpoint`; the server decides which events count.
// Pageviews and named clicks go to `endpoint`, the rows the script adds by itself
// (engage, out, download) to `endpoint + '/v2'`, which a host that predates them
// answers 404 instead of counting them as visits.
(function () {
  var endpoint = __ENDPOINT__;
  var waiting = 0;

  // The page's address without its fragment, which can carry a sign-in token.
  function address() {
    return location.origin + location.pathname + location.search;
  }

  // A random id per event, kept only in memory, so the server counts a retry once.
  function newId() {
    try {
      var bytes = crypto.getRandomValues(new Uint8Array(8)), hex = '';
      for (var i = 0; i < 8; i++) hex += (bytes[i] | 256).toString(16).slice(1);
      return hex;
    } catch (_) {
      return null;
    }
  }

  // No request headers, so the POST needs no CORS preflight. A busy host (429,
  // 503) or a failed network gets two more tries, after Retry-After (at most
  // 30 s) or 2 s then 4 s, with at most ten tries waiting at once.
  function send(body, attempt) {
    function again(seconds) {
      if (attempt < 2 && waiting < 10) {
        waiting++;
        setTimeout(function () { waiting--; send(body, attempt + 1); }, Math.min(seconds || 2 << attempt, 30) * 1e3);
      }
    }
    fetch(body.kind ? endpoint + '/v2' : endpoint, { method: 'POST', body: JSON.stringify(body), keepalive: true }).then(function (res) {
      if (res.status == 429 || res.status == 503) again(+res.headers.get('Retry-After'));
    }, function () { again(0); });
  }

  // Without an id a retry would be counted again, so such an event is sent once.
  function record(body) {
    body.id = newId();
    send(body, body.id ? 0 : 2);
  }

  // Engagement: one event when the reader leaves a page (it is hidden or
  // unloaded, whichever comes first), with the deepest quarter reached and the
  // time on screen in one of five ranges, nothing finer.
  var onScreenMs = 0, shownAt = 0, deepest = 0, engageSent = true;

  function measureDepth() {
    var height = document.documentElement.scrollHeight;
    deepest = Math.max(deepest, height > 0 ? Math.min(1, (scrollY + innerHeight) / height) : 1);
  }
  function startClock() {
    shownAt = document.visibilityState == 'visible' ? Date.now() : 0;
  }
  function stopClock() {
    if (shownAt) { onScreenMs += Date.now() - shownAt; shownAt = 0; }
  }
  function timeRange(seconds) {
    return seconds < 10 ? '<10s' : seconds < 30 ? '10-30s' : seconds < 120 ? '30s-2m' : seconds < 600 ? '2-10m' : '>10m';
  }
  function leave() {
    stopClock();
    if (engageSent) return;
    engageSent = true;
    measureDepth();
    record({ url: address(), kind: 'engage', scroll: Math.max(25, Math.ceil(deepest * 4) * 25), visible: timeRange(onScreenMs / 1e3) });
  }

  function pageview() {
    onScreenMs = 0; deepest = 0; engageSent = false;
    measureDepth();
    startClock();
    record({ url: address(), referrer: document.referrer || null, screen_width: innerWidth });
  }

  pageview();
  // A page restored from the back-forward cache is a new visit with its own engagement.
  addEventListener('pageshow', function (e) { if (e.persisted) pageview(); });
  addEventListener('scroll', measureDepth, { passive: true });
  document.addEventListener('visibilitychange', function () {
    if (document.visibilityState == 'hidden') leave(); else startClock();
  });
  addEventListener('pagehide', leave);

  // A click, or a middle click, on an element marked data-track is one named
  // click. On an unmarked link: to another host, only that host is sent (out);
  // to a file on this host (by extension, or with `download`), only its path.
  var FILE = /\.(pdf|zip|dmg|exe|msi|pkg|epub|mobi|mp3|mp4|m4a|mov|wav|docx|xlsx|pptx|odt|csv|txt|json|gz|tar|7z|apk)$/i;
  function onClick(e) {
    if (e.type == 'auxclick' && e.button != 1) return;
    var tracked = e.target.closest && e.target.closest('[data-track]');
    if (tracked) return record({ url: address(), event: tracked.getAttribute('data-track') });
    var link = e.target.closest && e.target.closest('a[href]');
    if (!link || !/^https?:$/.test(link.protocol)) return;
    if (link.host != location.host) record({ url: address(), kind: 'out', event: link.hostname });
    else if (link.hasAttribute('download') || FILE.test(link.pathname)) record({ url: address(), kind: 'download', event: link.pathname });
  }
  document.addEventListener('click', onClick);
  document.addEventListener('auxclick', onClick);
})();
