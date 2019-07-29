package dev.nhh.echoclient.network;

import dev.nhh.echoclient.audio.microphone.Microphone;
import dev.nhh.echoclient.audio.speaker.Speaker;

import java.io.IOException;
import java.net.DatagramSocket;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.logging.Level;
import java.util.logging.Logger;

public class Connection implements Runnable {

    private final URI uri;
    private final Logger logger = Logger.getLogger(Connection.class.getName());

    public Connection(String url) {
        this.logger.log(Level.INFO, "Connecting to " + url);
        this.uri = URI.create(url);
    }

    @Override
    public void run() {

        DatagramSocket socket;

        try {
            socket = new DatagramSocket();
        } catch (IOException e) {
            e.printStackTrace();
            return;
        }

        connect(socket);

        Microphone.INSTANCE.start(socket);
        Speaker.INSTANCE.start(socket);
    }

    private void connect(DatagramSocket socket) {
        HttpRequest request = HttpRequest.newBuilder()
                .uri(uri)
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

}
