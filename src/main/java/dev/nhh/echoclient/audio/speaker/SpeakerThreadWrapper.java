package dev.nhh.echoclient.audio.speaker;

import dev.nhh.echoclient.util.ThreadWrapper;

import java.util.logging.Level;
import java.util.logging.Logger;

public class SpeakerThreadWrapper extends ThreadWrapper {

    private final Logger logger = Logger.getLogger("SpeakerThreadWrapper");

    public SpeakerThreadWrapper() {
        super("speakerThreadWrapper", true);
    }

    @Override
    public void run() {
        logger.log(Level.INFO, this.getName() + " is started!");
        final var queue = SpeakerQueue.INSTANCE.getQueue();
        final var speaker = Speaker.INSTANCE;

        while(this.isRunning.get()) {
            while(!queue.isEmpty() && this.isRunning.get()) {
                speaker.write(queue.poll());
            }
        }

    }
}
