package dev.nhh.echo.util;

import java.util.concurrent.ConcurrentHashMap;

public enum ThreadScheduler {

    INSTANCE;

    private final ConcurrentHashMap<String, EchoThread> threads = new ConcurrentHashMap<>();

    public void closeAll() {
        this.threads
                .values()
                .forEach(EchoThread::stop);
    }

    public void start(EchoThread value) {
        // Check if there is already a reference and stop it.
        value.start();
        this.threads.putIfAbsent(value.getName(), value);
    }

    public void stop(String threadName) {
        final var thread = this.threads.get(threadName);
        thread.stop();
    }

}
