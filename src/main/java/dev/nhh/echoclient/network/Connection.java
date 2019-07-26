package dev.nhh.echoclient.network;

import dev.nhh.echoclient.audio.microphone.Microphone;
import dev.nhh.echoclient.audio.speaker.Speaker;
import dev.nhh.echoclient.util.ThreadScheduler;

import java.io.IOException;
import java.net.DatagramSocket;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;

public class Connection implements Runnable {

    @Override
    public void run() {

        DatagramSocket socket;

        try {
            socket = new DatagramSocket();
        } catch (IOException e) {
            e.printStackTrace();
            return;
        }

        setupMicrophone(socket);
        setupSpeakers(socket);

        HttpRequest request = HttpRequest.newBuilder()
                .uri(URI.create("http://116.203.73.169:4567/join"))
                .timeout(Duration.ofMinutes(1))
                .header("Content-Type", "application/json")
                .header("Port", String.valueOf(socket.getLocalPort()))
                .POST(HttpRequest.BodyPublishers.ofString(""))
                .build();

        HttpClient client = HttpClient.newBuilder().build();

        try {
            client.send(request, HttpResponse.BodyHandlers.ofString());
        } catch (IOException | InterruptedException e) {
            e.printStackTrace();
        }

    }

    private void setupSpeakers(DatagramSocket socket) {
        var speaker = new Speaker(socket);
        var t = new Thread(speaker);
        t.setDaemon(true);
        t.start();
        ThreadScheduler.INSTANCE.addThread(t);
    }

    private void setupMicrophone(DatagramSocket socket) {
        var mic = new Microphone(socket);
        var t = new Thread(mic);
        t.setDaemon(true);
        t.start();
        ThreadScheduler.INSTANCE.addThread(t);
    }

}
