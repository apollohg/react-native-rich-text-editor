#import "PreparedProseViewerFabricEventHarness.h"

#import <objc/message.h>

#import "../Viewer/Fabric/PREPPreparedProseViewerComponentView.h"
#import "ReactNativeProseEditor-Swift.h"

#include <react/renderer/components/PreparedProseViewer/PreparedProseMeasurementsManager.h>
#include <react/renderer/components/PreparedProseViewer/PreparedProseViewerShadowNode.h>
#include <react/renderer/components/ReactNativeProseEditorSpec/EventEmitters.h>
#include <react/renderer/components/ReactNativeProseEditorSpec/Props.h>
#include <react/renderer/core/ConcreteState.h>
#include <react/renderer/core/EventEmitter.h>
#include <react/renderer/core/LayoutMetrics.h>

@interface PREPPreparedProseViewerComponentView (AtomEventTesting)
- (void)prep_setAtomEventSink:(void (^)(NSDictionary<NSString *, id> *event))sink;
@end

using namespace facebook::react;

@implementation PREPPreparedProseViewerFabricEventHarness {
  UIWindow *_window;
  UIView *_host;
  PREPPreparedProseViewerComponentView *_component;
  PREPPreparedProseDrawingView *_drawing;
  NSMutableArray<NSDictionary<NSString *, id> *> *_events;
  std::shared_ptr<PreparedProseViewerProps> _mutableProps;
  PreparedProseViewerState _stateData;
  PreparedProseViewerShadowNode::ConcreteState::Shared _state;
  Props::Shared _props;
  EventEmitter::Shared _emitter;
  LayoutMetrics _metrics;
  CGFloat _scale;
}

- (instancetype)initWithSource:(NSString *)source
                     configJSON:(NSString *)configJSON
                      themeJSON:(NSString *)themeJSON
                      surfaceID:(int64_t)surfaceID
                    componentTag:(int64_t)componentTag
                     leaseHandle:(uint64_t)leaseHandle
                           width:(CGFloat)width
                           scale:(CGFloat)scale
{
  if (self = [super init]) {
    _scale = scale;
    _events = [NSMutableArray new];
    _mutableProps = std::make_shared<PreparedProseViewerProps>();
    _mutableProps->sourceKind = PreparedProseViewerSourceKind::Json;
    _mutableProps->source = std::string(source.UTF8String ?: "");
    _mutableProps->configJson = std::string(configJSON.UTF8String ?: "{}");
    _mutableProps->themeJson = std::string(themeJSON.UTF8String ?: "");
    _mutableProps->imagesEnabled = true;
    _mutableProps->collapsesWhenEmpty = true;
    _mutableProps->enableLinkTaps = true;
    _props = _mutableProps;
    _stateData.leaseHandle = leaseHandle;
    _stateData.surfaceId = static_cast<SurfaceId>(surfaceID);
    _stateData.componentTag = static_cast<Tag>(componentTag);
    _stateData.leaseLifecycle = std::make_shared<PreparedProseViewerLeaseLifecycle>();
    [self rebuildState];
    [self prepareCurrentArtifactAtWidth:width];

    _window = [[UIWindow alloc] initWithFrame:CGRectMake(0, 0, width, 240)];
    _host = [[UIView alloc] initWithFrame:_window.bounds];
    _component = [[PREPPreparedProseViewerComponentView alloc] initWithFrame:_host.bounds];
    _emitter = std::make_shared<PreparedProseViewerEventEmitter>(
        SharedEventTarget{}, EventDispatcher::Weak{});
    [_component updateEventEmitter:_emitter];
    SEL selector = @selector(prep_setAtomEventSink:);
    if ([_component respondsToSelector:selector]) {
      __weak PREPPreparedProseViewerFabricEventHarness *weakSelf = self;
      using SetSink = void (*)(id, SEL, void (^)(NSDictionary<NSString *, id> *));
      ((SetSink)objc_msgSend)(_component, selector, ^(NSDictionary<NSString *, id> *event) {
        PREPPreparedProseViewerFabricEventHarness *strongSelf = weakSelf;
        if (!strongSelf) return;
        [strongSelf->_events addObject:event];
      });
    }
    [_window addSubview:_host];
    [_host addSubview:_component];
    _drawing = (PREPPreparedProseDrawingView *)_component.subviews.firstObject;
    _window.hidden = NO;
    [_component updateProps:_props oldProps:Props::Shared{}];
    [_component updateState:_state oldState:State::Shared{}];
    [self installMetricsWithWidth:width];
  }
  return self;
}

- (void)dealloc
{
  [_component prep_setAtomEventSink:nil];
  [_component prepareForRecycle];
  _window.hidden = YES;
}

- (NSArray<NSDictionary<NSString *,id> *> *)events { return [_events copy]; }
- (UIView *)drawingView { return _drawing; }

- (void)expireLease
{
  [[PREPPreparedProseLayoutRegistry sharedRegistry]
      releaseFabricLeaseSurfaceId:_stateData.surfaceId
                    componentTag:_stateData.componentTag
                     leaseHandle:_stateData.leaseHandle];
  _stateData.leaseLifecycle->deactivate();
}

- (void)replaceLease:(uint64_t)leaseHandle
{
  _stateData.leaseHandle = leaseHandle;
  _stateData.leaseLifecycle = std::make_shared<PreparedProseViewerLeaseLifecycle>();
  [self rebuildState];
  [self prepareCurrentArtifactAtWidth:_metrics.getContentFrame().size.width];
  [_component updateState:_state oldState:State::Shared{}];
  [self installMetricsWithWidth:_metrics.getContentFrame().size.width];
}

- (void)setTableLogicalOffset:(CGFloat)offset sourceIdentity:(NSString *)sourceIdentity
{
  [_drawing setTableLogicalOffset:offset sourceIdentity:sourceIdentity];
}

- (void)setHostHidden:(BOOL)hidden { _host.hidden = hidden; }

- (void)setLayoutWidth:(CGFloat)width { [self installMetricsWithWidth:width]; }

- (void)beginPendingReplacementWithWidth:(CGFloat)width
{
  _stateData.attachmentRevision += 1;
  [self rebuildState];
  [_component updateState:_state oldState:State::Shared{}];
  [self installMetricsWithWidth:width];
}

- (void)prepareReplacementAndInstall
{
  [self prepareCurrentArtifactAtWidth:_metrics.getContentFrame().size.width];
  [self installMetricsWithWidth:_metrics.getContentFrame().size.width];
}

- (void)recycle { [_component prepareForRecycle]; }

- (void)rebuildState
{
  _state = std::make_shared<PreparedProseViewerShadowNode::ConcreteState>(
      std::make_shared<const PreparedProseViewerState>(_stateData),
      ShadowNodeFamily::Weak{});
}

- (void)prepareCurrentArtifactAtWidth:(CGFloat)width
{
  PreparedProseMeasurementsManager manager{nullptr};
  manager.bindLeaseLifecycle(_stateData.surfaceId, _stateData.componentTag,
      _stateData.leaseHandle, _stateData.leaseLifecycle);
  (void)manager.measure(_stateData.surfaceId, _stateData.componentTag, *_mutableProps,
      width, _scale, _stateData.attachmentRevision, _stateData.nativeFontRevision,
      _stateData.nativeFontScale, _stateData.userInterfaceStyle,
      _stateData.accessibilityContrast, _mutableProps->fontEnvironmentRevision,
      _stateData.leaseHandle, _stateData.leaseLifecycle);
}

- (void)installMetricsWithWidth:(CGFloat)width
{
  LayoutMetrics next{};
  next.frame = {{0, 0}, {static_cast<Float>(width), 240}};
  next.pointScaleFactor = _scale;
  const auto previous = _metrics;
  _metrics = next;
  [_component updateLayoutMetrics:next oldLayoutMetrics:previous];
}

@end
