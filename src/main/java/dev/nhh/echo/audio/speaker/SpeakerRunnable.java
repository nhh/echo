package dev.nhh.echo.audio.speaker;

import dev.nhh.echo.util.EchoThread;

import java.util.logging.Level;
import java.util.logging.Logger;

public class SpeakerRunnable extends EchoThread {

    private final Logger logger = Logger.getLogger("SpeakerRunnable");

    public SpeakerRunnable(String name) {
        super(name);
    }

    @Override
    public void run() {
        logger.log(Level.INFO, this.getName() + " is started!");
        final var queue = SpeakerQueue.INSTANCE.getQueue();
        final var speaker = Speaker.INSTANCE;

        while(this.isRunning.get()) {
            while(!queue.isEmpty()) {
                speaker.write(queue.poll());
            }
        }

    }
}
