// Link-time hooks of the Apple shell (Task B7). MidgardKit calls each hook; this target
// defines it weakly, as "nothing registered". A library that a target links may define
// the same symbol strongly, and the linker then uses that definition: what is
// registered follows what is linked, with no runtime switch and no lookup by name.
// [FR-C-1]
#ifndef MIDGARD_HOOKS_H
#define MIDGARD_HOOKS_H

/// The edition provider (banner, About, Heimdall's 3D view): NULL here, so a release
/// app has none; the 3D editions' library returns an unretained pointer to its provider,
/// which lives for the whole process.
const void *_Nullable midgard_edition_hook(void);

#endif
