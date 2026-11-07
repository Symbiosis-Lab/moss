// The visit counter of a site published to the moss host. The build writes the
// site's beacon address into `endpoint`; the server decides which events count.
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
    fetch(endpoint, { method: 'POST', body: JSON.stringify(body), keepalive: true }).then(function (res) {
      if (res.status == 429 || res.status == 503) again(+res.headers.get('Retry-After'));
    }, function () { again(0); });
  }

  // Without an id a retry would be counted again, so such an event is sent once.
  function record(body) {
    body.id = newId();
    send(body, body.id ? 0 : 2);
  }

  function pageview() {
    record({ url: address(), referrer: document.referrer || null, screen_width: innerWidth });
  }

  pageview();
  // A page restored from the back-forward cache is a new visit.
  addEventListener('pageshow', function (e) { if (e.persisted) pageview(); });
  document.addEventListener('click', function (e) {
    var tracked = e.target.closest && e.target.closest('[data-track]');
    if (tracked) record({ url: address(), event: tracked.getAttribute('data-track') });
  });
})();
