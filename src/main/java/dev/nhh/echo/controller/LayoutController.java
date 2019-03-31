package dev.nhh.echo.controller;

import dev.nhh.echo.audio.speaker.SpeakerRunnable;
import dev.nhh.echo.client.Client;
import dev.nhh.echo.util.ThreadScheduler;
import javafx.fxml.FXML;
import javafx.scene.control.TextField;
import javafx.scene.input.MouseEvent;

public class LayoutController {

    private final Client client = new Client("localhost");
    private final SpeakerRunnable speakerRunnable = new SpeakerRunnable("speakerRunnable");

    @FXML
    private TextField textField;

    @FXML
    protected void startMicrophone(MouseEvent event) {
        ThreadScheduler.INSTANCE.start(client);
    }

    @FXML
    protected void stopMicrophone(MouseEvent event) {
        ThreadScheduler.INSTANCE.stop("clientThread");
    }


    @FXML
    protected void start(MouseEvent event) {
        ThreadScheduler.INSTANCE.start(speakerRunnable);
    }

    @FXML
    protected void stop(MouseEvent event) {
        ThreadScheduler.INSTANCE.stop("speakerThread");
    }

}
