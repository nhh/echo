package dev.nhh.echoclient.util;

import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;

public enum ThreadScheduler {

    INSTANCE;

    private final ConcurrentHashMap<UUID, Thread> threads = new ConcurrentHashMap<>();

    public void stopAll() {
        this.threads
                .values()
                .forEach(Thread::interrupt);
    }

    public void startAll() {
        this.threads
                .values()
                .forEach(Thread::run);
    }

    public UUID addThread(Thread thread) {
        var id = UUID.randomUUID();
        this.threads.putIfAbsent(id, thread);
        return id;
    }

    public void start(UUID uuid) {
        final var thread = this.threads.get(uuid);
        if(thread.isAlive()) return;
        thread.run();
    }

    public void stop(UUID uuid) {
        final var thread = this.threads.get(uuid);
        if(thread.isInterrupted()) return;
        thread.interrupt();
    }

}
