# Echo

### Introduction

Echo was all time meant to be a tool like teamspeak. The main goal was, to create a more feature rich and modern approach of teamspeak.
Something that is more like Discord, but not at all. Originally i tried several programming languages, javascript, python, java, ruby - all in combination with http frameworks. The communication should be done via webrtc.
But the reality shows, that webrtc is not suited yet for what i needed. And i decided to go with java as my primary programming language.

So, the main goal of this project is still learning and getting deeper insights of java. Some of the topics i want to cover here:

1. Multithreading and IPC
2. Working with primitives and performance optimized code
3. "low" level networking (udp, tcp)
4. Architecture skills
5. Audio codecs / compression / multiple input lines
6. Best practises for concurrent code
    - Enum singletons
    - Concurrent Java packages
7. Application profiling
8. Not relying on HTTP as primary transport protocol
    - https://grpc.io/
    - https://msgpack.org/
    - http://rsocket.io/

For now i am not using any UI lib, because that is out of scope for now. The goal is to create a reliable sound engine.


### Engine
