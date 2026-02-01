PROMPT === [EXIT] Start ===
SELECT 1 AS before_exit FROM dual;
PROMPT About to EXIT. Anything after this should not run.
EXIT
PROMPT ERROR: This should not appear.
SELECT 1 AS after_exit FROM dual;
