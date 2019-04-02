package dev.nhh.echoclient.util;

import java.util.concurrent.atomic.AtomicBoolean;

public abstract class ThreadWrapper {

    private Thread thread;
    private final boolean daemon;
    private final String name;
    protected final AtomicBoolean isRunning = new AtomicBoolean(false);

    public ThreadWrapper(String name, boolean daemon) {
        this.name = name;
        this.daemon = daemon;
    }

    public void start() {
        if(isRunning.get()) return;
        this.isRunning.set(true);
        thread = new Thread(this::run);
        thread.setDaemon(daemon);
        this.thread.setName(name);
        this.thread.start();
    }

    public void stop() {
        if(!isRunning.get()) return;
        this.isRunning.set(false);
        this.thread.interrupt();
    }

    public void run() {
        throw new IllegalStateException("Not yet implemented");
    }

    protected String getName() {
        return name;
    }

}
