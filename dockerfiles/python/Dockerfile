# Python Execution Environment
FROM python:3.11-slim

WORKDIR /code

# Install common packages
RUN pip install --no-cache-dir \
    pytest \
    numpy \
    requests

# Create runner script
COPY dockerfiles/python/runner.py /runner.py

# Default command runs the runner
CMD ["python", "/runner.py"]
