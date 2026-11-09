public class AuditTracker {
    public static int record_metric(int prev, int next_val) {
        return (prev + next_val) / 2;
    }
}
