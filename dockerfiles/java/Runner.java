import java.io.*;
import java.util.Base64;

/**
 * Optimus Java Runner
 * Executes user code with test input in a sandboxed environment
 */
public class Runner {
    public static void main(String[] args) {
        try {
            // Read base64-encoded source code and input from environment
            String sourceCodeB64 = System.getenv("SOURCE_CODE");
            String testInputB64 = System.getenv("TEST_INPUT");
            
            if (sourceCodeB64 == null || sourceCodeB64.isEmpty()) {
                System.err.println("Error: SOURCE_CODE environment variable not set");
                System.exit(1);
            }
            
            // Decode source code and input
            String sourceCode = new String(Base64.getDecoder().decode(sourceCodeB64));
            String testInput = testInputB64 != null && !testInputB64.isEmpty() 
                ? new String(Base64.getDecoder().decode(testInputB64)) 
                : "";
            
            // Write source code to Main.java
            try (FileWriter writer = new FileWriter("/code/Main.java")) {
                writer.write(sourceCode);
            }
            
            // Compile the code
            ProcessBuilder compileBuilder = new ProcessBuilder("javac", "/code/Main.java");
            compileBuilder.directory(new File("/code"));
            compileBuilder.redirectErrorStream(true);
            Process compileProcess = compileBuilder.start();
            
            String compileOutput = readStream(compileProcess.getInputStream());
            int compileExitCode = compileProcess.waitFor();
            
            if (compileExitCode != 0) {
                System.err.println("Compilation error:");
                System.err.println(compileOutput);
                System.exit(1);
            }
            
            // Execute the compiled code
            ProcessBuilder runBuilder = new ProcessBuilder("java", "-cp", "/code", "Main");
            runBuilder.directory(new File("/code"));
            Process runProcess = runBuilder.start();
            
            // Write test input to stdin
            if (!testInput.isEmpty()) {
                try (OutputStream stdin = runProcess.getOutputStream();
                     OutputStreamWriter writer = new OutputStreamWriter(stdin)) {
                    writer.write(testInput);
                    writer.flush();
                }
            }
            
            // Read output
            String stdout = readStream(runProcess.getInputStream());
            String stderr = readStream(runProcess.getErrorStream());
            
            int exitCode = runProcess.waitFor();
            
            // Print outputs
            System.out.print(stdout);
            if (!stderr.isEmpty()) {
                System.err.print(stderr);
            }
            
            System.exit(exitCode);
            
        } catch (Exception e) {
            System.err.println("Runner error: " + e.getMessage());
            e.printStackTrace();
            System.exit(1);
        }
    }
    
    private static String readStream(InputStream stream) throws IOException {
        StringBuilder output = new StringBuilder();
        try (BufferedReader reader = new BufferedReader(new InputStreamReader(stream))) {
            String line;
            while ((line = reader.readLine()) != null) {
                output.append(line).append("\n");
            }
        }
        return output.toString();
    }
}
