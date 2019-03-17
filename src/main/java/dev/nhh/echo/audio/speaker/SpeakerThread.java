package dev.nhh.echo.audio.speaker;

import java.util.concurrent.TimeUnit;

public class SpeakerThread implements Runnable {

    @Override
    public void run() {
        final var queue = SpeakerQueue.INSTANCE.getQueue();
        final var speaker = Speaker.INSTANCE;

        for(;;) {
            while(!queue.isEmpty()) {
                speaker.write(queue.poll());
            }

            try {
                TimeUnit.MILLISECONDS.sleep(10);
            } catch (InterruptedException e) {
                e.printStackTrace();
            }
        }

    }


}
