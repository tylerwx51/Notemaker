# ADR 0024: Session local GIF playback

GIFs are immutable embedded document objects. The shared model validates their
dimensions, byte size, frame count, and structure. The CLI can add a GIF with
structured placement, and the server validates the same object before accepting
it. Static renderers draw the first frame.

Each Android Activity owns its selected GIF, current frame, and play state. The
player starts paused, follows source frame delays with a 100 ms minimum, and
pauses when the Activity leaves the foreground. Control actions are exposed
through `UiActions`; they are not document operations and do not enter the
journal. This lets two apps view and control the same GIF independently.

The player draws the active frame over the static canvas tile and a control bar
below each visible GIF. The bar includes previous frame, play/pause, next frame,
and a timeline. The toolbar's Interact mode routes touches to those controls;
other tools retain their drawing and selection behavior. The bar belongs to the
Activity session, not the document. The redraw is limited to the GIF and its
controls and only scheduled during playback. E Ink refresh quality and
comfortable frame timing require physical tablet review.
