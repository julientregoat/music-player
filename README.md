## **work in progress!**

a music player + library born out of a desire to enable deeper music classification through user defined properties, to execute complex queries using those properties, and to support 'decentralized libraries'.

the database and audio playback are handled by the `librarian` package. it's designed to expose an API to be called by frontend implementations. the proof of concept is done here, but error handling needs to be implemented and the public api needs to be refined.

currently, the first ui in progress is using `gtk-rs` - this was chosen for cross platform compatibility.

down the line, I plan to eventually do a dedicated macos ui using swift ui. eventually, I'd also like to target mobile platforms, especially Linux based phones.
