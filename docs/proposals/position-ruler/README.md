# A position ruler for a row along an ordered axis

**Status:** Proposed. Not scheduled: one site needs it so far, and moss already gives that site what it needs to draw one itself.

## What it is

A scroll row (`:::grid N {scroll}`) shows where the reader is with dots, or with "12 / 40" between ‹ and › buttons once a row has more than seven cards. Both count cards. Some rows are a series along an axis that is not the card count: prints of the stations along a road, placed by distance; letters placed by date; chapters placed by page. For those, a ruler reads better than a count. It is a line with a mark at each card's position on the axis, the ends labelled ("0 · Harbour", "490 km · Capital"), and the current card marked and named. It is also a slider: a tap, a drag or an arrow key moves the row to the card nearest that point.

## What moss does today

The row fires `moss-scroll-position` with the current card, the cards in view and the count, and a theme can hide the built-in indicator for one row (`.moss-grid.timeline + .moss-scroll-dots { display: none; }`). That is enough to draw a ruler in a theme script: the site that asked for this draws its own in about seventy lines, from a data file of positions its own tooling writes.

## Why it is not built yet

- One site uses it. A second, different series would show which parts are general (the axis, the labels, the slider) and which belong to the first site (its distances, its bands of named stretches).
- The general part needs a position for every card, and the one site's positions are not in its pages: its distances come from a data file its own tooling writes. moss already has the place to put them, a declared field of kind `number`, `count` (`48.1 km`) or `date` on each page, so a ruler should read one of those rather than a new key. The site would have to move its distances into its pages first, and until it does, a built-in ruler has no user.
- Prior art keeps time out of the row. Collection tools draw a separate timeline view (TimelineJS in CollectionBuilder, Neatline Time on SIMILE Timeline in Omeka). Carousels mark position with dots or tabs, and CSS `::scroll-marker` is arriving to do that natively. A ruler that is also a slider is closest to a media scrubber, and should follow the ARIA slider pattern: `role="slider"`, a value and a text value naming the card, and arrow, Home and End keys.

## The likely shape, when a second site needs it

`:::grid 4 {scroll ruler=distance}` on a row of page cards: `ruler=` names a declared `number`, `count` or `date` field. Each card's mark sits at its page's value along the row's span, and the ends are labelled with the first and last values and titles. The current card is named in the slider's text value, and the dots are dropped for that row. Without a script the row still scrolls and the ruler is not drawn.
