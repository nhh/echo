package dev.nhh.echo.controller;

import dev.nhh.echo.audio.speaker.SpeakerThreadWrapper;
import dev.nhh.echo.client.ClientThreadWrapper;
import dev.nhh.echo.util.ThreadScheduler;
import javafx.fxml.FXML;
import javafx.scene.control.TextField;
import javafx.scene.input.MouseEvent;

public class LayoutController {

    @FXML
    private TextField textField;

    @FXML
    protected void startMicrophone(MouseEvent event) {
        ThreadScheduler.INSTANCE.addWrapper(new ClientThreadWrapper("localhost"));
        ThreadScheduler.INSTANCE.start("clientThreadWrapper");
    }

    @FXML
    protected void stopMicrophone(MouseEvent event) {
        ThreadScheduler.INSTANCE.stop("clientThreadWrapper");
    }

    @FXML
    protected void start(MouseEvent event) {
        ThreadScheduler.INSTANCE.addWrapper(new SpeakerThreadWrapper());
        ThreadScheduler.INSTANCE.start("speakerThreadWrapper");
    }

    @FXML
    protected void stop(MouseEvent event) {
        ThreadScheduler.INSTANCE.stop("speakerThreadWrapper");
    }

}
