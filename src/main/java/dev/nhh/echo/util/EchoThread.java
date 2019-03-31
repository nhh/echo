package dev.nhh.echo.util;

import java.util.concurrent.atomic.AtomicBoolean;

public abstract class EchoThread extends Thread {

    private Thread thread;
    private final String name;
    protected final AtomicBoolean isRunning = new AtomicBoolean(false);

    public EchoThread(String name) {
        this.name = name;
    }

    public void start() {
       this.isRunning.set(true);
       this.thread = new Thread(this);
    }

}
